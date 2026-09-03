//! # Snapping engine (Pass 12.M1 — decision 011 §2.2)
//!
//! A **read-only, GUI-free geometry service** that, given a query point in
//! **page space** (genuine PDF default user space, §8.3.2.3) and a page's
//! already-decomposed [`PageObjects`] (Pass 9a), returns every snap
//! candidate within a caller-supplied tolerance — a marker point the
//! operator's pick can lock onto. It is the substrate of decision 011's
//! first beta measurement/dimensioning tool, and it is deliberately built as
//! a **shared service**: the SAME engine feeds the Pass 12.M2 dimension
//! tools (pick point A, pick point B) AND the future Pass 9c-min node-drag
//! (snap a dragged anchor onto another object's geometry). Neither caller is
//! privileged; the API takes a query point + a config + the object model and
//! knows nothing about tools.
//!
//! ## Where the math lives (GUI-core separation, binding invariant)
//!
//! Everything here is `pdfcer-core`, with **no GUI/windowing dependency**
//! (no egui/eframe/winit/wgpu, not even `tiny-skia`). The snap MATH — target
//! enumeration, priority, tie-breaking, tolerance semantics, H/V projection,
//! neighbourhood-bounded intersection — is pure geometry over the Pass 9a
//! object model. The GUI (`pdfce-gui`) owns exactly two things this module
//! deliberately does NOT: (1) converting a fixed **screen-space** pixel
//! tolerance into the page-space [`SnapConfig::tolerance`] via the current
//! zoom (that is what makes snap "feel" zoom-invariant — the conversion, not
//! the engine), and (2) rendering the fuzzy indicator glyph + type label.
//! `cargo tree -p pdfcer-core` stays egui/eframe/winit/wgpu/glow-free
//! (`docs/ARCHITECTURE.md` §3).
//!
//! ## The seven-level fixed priority — a DELIBERATE simplification (R61)
//!
//! Decision 011 §2.2 commits pdfcer to a **fixed** high→low priority list of
//! snap targets, and this module implements exactly that list (see
//! [`SnapKind::priority`]). This is a **deliberate divergence** from
//! Inkscape's evolved (1.4-era) model, which is a per-category *toggle* plus
//! a two-tier *precedence* concept, NOT a single fixed priority order
//! (`D:\Dev\Rag-Specialized\Inkscape_Features\snapping__engine_targets_
//! sources.md`: "Inkscape's snapping model, as of 1.4, is NOT simply
//! 'everything on, ties broken by distance' but has grown an explicit
//! two-tier precedence concept"). pdfcer chooses the simpler fixed list on
//! purpose — it is deterministic, needs no per-category UI, and is enough for
//! the measurement beta. This note exists so a future reader does not
//! "restore" a toggle model believing the fixed list was an oversight; it was
//! decided (inkscape-librarian flag, decision 011 §2.2).
//!
//! ## Bounding-box-corner snapping is OUT for the beta (documented divergence)
//!
//! Inkscape treats an object's four bounding-box corners as a first-class
//! snap source/target (its own `snapping__engine_targets_sources.md` names it
//! `should_have`). Decision 011's committed seven-level list has **no**
//! bbox-corner entry, and this module follows decision 011: bbox-corner
//! snapping is a **named fast-follow**, not silently added (fuzzy-never-sneaky
//! applies to scope too — adding an un-decided target type would be a sneaky
//! divergence from the committed list). When it lands it becomes a new
//! [`SnapKind`] variant between `Midpoint` and `Intersection` (an object-level
//! target, coarser than a node) — flagged here, not built.
//!
//! ## Intersection snapping: DEFAULT OFF + neighbourhood-bounded (Z4)
//!
//! Segment–segment intersection is the one target category with a real,
//! independently-corroborated performance ceiling: Inkscape ships it OFF by
//! default *specifically because* an all-pairs intersection scan freezes on
//! dense pages (`snapping__priority_tolerance_limits.md`: "Path intersection
//! snapping is turned off by default in Inkscape because it is extremely slow
//! in large documents … freezing/hanging when … too many snap targets on
//! screen"). pdfcer mirrors that precedent exactly ([`SnapConfig::intersections`]
//! defaults `false`) and, when enabled, computes intersections **only among
//! segments whose bounding box is within the tolerance neighbourhood of the
//! query point** (a spatial pre-filter — [`near_query_segments`]), never
//! globally, with a hard cap ([`MAX_NEIGHBOURHOOD_SEGMENTS`]) so even a
//! hostile page with an enormous tolerance cannot force an O(n²) scan. This is
//! decision 011 Appendix A's Z4 mitigation, grounded in the Inkscape finding.
//!
//! ## Fuzzy, never sneaky (rule 4)
//!
//! This engine only *reports* candidates; it commits nothing. The GUI shows
//! the current candidate's marker + type label BEFORE the click lands, and
//! the operator cycles/overrides. The one candidate that is a genuine
//! *inference about operator intent* rather than a fact about existing
//! geometry — [`SnapKind::DerivedCenterline`], a filled thin quad's derived
//! midline (Pass 9a [`CenterlineCandidate`]) — is a distinct kind precisely so
//! the GUI can give it a visually distinct glyph and the extra two-click
//! confirm decision 011 requires (§2.1 "fuzzy inference … never
//! auto-committed"). Routine kinds (node/endpoint/center/…) are deterministic
//! facts about geometry already on the page: the indicator IS the disclosure,
//! no second confirm.
//!
//! ## Zoom-invariant tolerance (decision 011 §2.2)
//!
//! [`SnapConfig::tolerance`] is a **page-space** distance. The GUI derives it
//! each frame from a fixed screen-space pixel radius (≈8–12 px) divided by the
//! current zoom, so a constant on-screen catch radius maps to a *shrinking*
//! page-space tolerance as the operator zooms in — the "feel" stays constant.
//! The engine takes the already-converted page-space value; the zoom-invariance
//! property is proven where the conversion lives (`pdfce-gui`'s
//! `viewer::screen_to_page` distance test + `canvas::screen_tolerance_to_page`).
//!
//! ## Marquee-vs-pan (the Pass 9a-owed resolution, confirmed here)
//!
//! Pass 9a made a canvas drag in no-tool selection mode a rubber-band
//! **marquee** (pan moved to wheel/scrollbars — the Inkscape/Illustrator
//! convention, R61). Pass 12.M1 KEEPS that model: the measurement tools this
//! engine serves use a **click-A-then-click-B** gesture (each a discrete snap
//! query at the pointer), never a drag-select, so there is no conflict — a
//! measure pick is a click, not a marquee drag. The two interaction models
//! are orthogonal (selection-mode marquee vs. tool-mode point picks) and the
//! ui-spec's interaction model governs; this engine is gesture-agnostic (it
//! answers a point query and does not know whether the point came from a
//! click, a drag, or a keyboard nudge).

use std::collections::HashSet;

use super::centerline::page_candidates;
use super::decompose::{PageObjects, PathObject, Segment, Subpath, VectorObject};
use super::geometry::{Bounds, Point};

/// Fixed cubic-flattening subdivision for on-segment projection and
/// intersection (the same fixed, bounded subdivision [`super::hit`] uses for
/// its own proximity math — 16 chords is sub-pixel at any realistic snap
/// tolerance and bounds the per-object work a hostile node count can force).
pub const SNAP_FLATTEN_STEPS: usize = 16;

/// Hard cap on how many segments the neighbourhood-bounded intersection scan
/// considers (the Z4 guard, decision 011 / Inkscape freeze precedent). Even
/// with an adversarially huge tolerance that pulls every segment into the
/// "neighbourhood," the pairwise scan is bounded to `N²` for `N` ≤ this — a
/// few tens of thousands of pair tests, not the millions an unbounded page
/// would force. Past this, intersection snapping is skipped for the query
/// (the other, cheaper target kinds still answer). 256² ≈ 65k pair tests is a
/// comfortable per-frame ceiling.
pub const MAX_NEIGHBOURHOOD_SEGMENTS: usize = 256;

/// Hard cap on the total candidate list length before sorting/dedup, so a
/// pathological page with an enormous tolerance cannot make the sort/dedup
/// itself unbounded. Far above any real snap neighbourhood (a handful of
/// targets), it only ever truncates adversarial input.
pub const MAX_CANDIDATES: usize = 4096;

/// Points closer than this (page units) are treated as the SAME location for
/// dedup — a shared vertex two subpaths both anchor, a midpoint that
/// coincides with an on-segment projection. Sub-pixel at any zoom (1/1000 pt),
/// so it collapses only genuinely coincident targets, never two distinct
/// snap points the operator might want to cycle between (those are ≥ a pixel
/// apart in page space at the tolerances snapping uses).
const DEDUP_QUANTUM: f64 = 1.0e-3;

/// The kind of geometry a [`SnapCandidate`] locks onto — decision 011 §2.2's
/// seven-level target list, plus the one derived-inference kind
/// ([`Self::DerivedCenterline`]) the GUI must render distinctly.
///
/// [`Self::priority`] encodes the fixed high→low order; the GUI maps each
/// variant to a glyph + a human label (those user-facing strings live in
/// `pdfce-gui`'s `ui_text`, never here — this crate stays UI-string-free).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapKind {
    /// A path anchor that is NOT a free terminus — an interior vertex of a
    /// polyline, or any anchor of a closed contour (decision 011 priority 1,
    /// "path nodes / anchor vertices"). The highest-priority target: a shared
    /// junction the operator almost always means to hit.
    Node,
    /// A **free terminus** of an *open* subpath (its first or last on-curve
    /// point, when the subpath is not closed) — decision 011 priority 2,
    /// "explicit segment endpoints." Distinguished from [`Self::Node`] because
    /// a lone open end (a drawn line's tip) reads as an "endpoint," while a
    /// coincident shared vertex reads as a "node" (dedup keeps the higher-
    /// priority `Node` when the two land on the same point).
    Endpoint,
    /// The centre of a circle/arc object (decision 011 priority 3, "circle/arc
    /// centers (derived, including best-fit)"). Pass 12.M1 supplies the object
    /// centre (the anchor bounding-box centre of a closed, curve-dominated
    /// path — exact for a circle or an ellipse, which are centrally symmetric).
    /// The least-squares **Taubin best-fit** centre over an arbitrary node set
    /// is the Pass 12.M2 upgrade; the kind is the same so a 12.M2 fit slots in
    /// without changing the snap surface.
    Center,
    /// The midpoint of a straight segment — an `l` edge or a subpath's closing
    /// edge (decision 011 priority 4, "segment midpoints"). Cubic-segment
    /// midpoints are a fast-follow (a Bézier's "midpoint" is ambiguous; the
    /// on-segment projection [`Self::SegmentCenterline`] already lets a pick
    /// land anywhere along a curve).
    Midpoint,
    /// A segment–segment crossing (decision 011 priority 5). **Off by default**
    /// and neighbourhood-bounded (module docs, Z4). Only present when
    /// [`SnapConfig::intersections`] is set.
    Intersection,
    /// The derived midline of a filled thin quad (a line drawn as a filled
    /// rectangle), from Pass 9a's [`CenterlineCandidate`]. A genuine **fuzzy
    /// inference** the operator must confirm — the GUI gives it a distinct
    /// glyph and a two-click confirm (decision 011 §2.1, ui-spec §2.3.1);
    /// placed just above [`Self::SegmentCenterline`] because it is a MORE
    /// specific inference about the same kind of geometry (a line's centre).
    DerivedCenterline,
    /// The nearest point on a routine (stroked-path) segment centerline — the
    /// perpendicular projection of the query onto a segment (decision 011
    /// priority 6, "nearest point on a segment centerline"). Zero inference:
    /// a stroked path's geometry IS its centerline (the stroke straddles it
    /// ±w/2), so this is a deterministic fact, not a derivation.
    SegmentCenterline,
    /// A page coordinate axis (`x = 0` / `y = 0`) or, if a grid is configured,
    /// a grid line / intersection (decision 011 priority 7, "page-axis /
    /// optional grid"). The lowest-priority fallback.
    Axis,
}

impl SnapKind {
    /// The fixed priority rank, **lower = higher priority** (decision 011
    /// §2.2's high→low list). Ties within a rank are then broken by distance
    /// to the query (nearest wins) — see [`snap_candidates`].
    ///
    /// The order (0 = highest): `Node` < `Endpoint` < `Center` < `Midpoint`
    /// < `Intersection` < `DerivedCenterline` < `SegmentCenterline` < `Axis`.
    /// `DerivedCenterline` sits just above `SegmentCenterline` (ui-spec §2.4:
    /// "just above segment-centerline, since it is a MORE specific inference
    /// about the SAME kind of geometry") but carries its confirm step
    /// regardless of list position.
    #[must_use]
    pub fn priority(self) -> u8 {
        match self {
            SnapKind::Node => 0,
            SnapKind::Endpoint => 1,
            SnapKind::Center => 2,
            SnapKind::Midpoint => 3,
            SnapKind::Intersection => 4,
            SnapKind::DerivedCenterline => 5,
            SnapKind::SegmentCenterline => 6,
            SnapKind::Axis => 7,
        }
    }

    /// Whether this kind is a **fuzzy inference about operator intent** that
    /// the GUI must render distinctly and gate behind an extra confirm
    /// (rule 4). Only [`Self::DerivedCenterline`] qualifies — every other kind
    /// is a deterministic fact about geometry already on the page.
    #[must_use]
    pub fn is_derived(self) -> bool {
        matches!(self, SnapKind::DerivedCenterline)
    }
}

/// One snap candidate: a target point in **page space**, its [`SnapKind`],
/// and the index of the object it came from (for a disclosure / to let the
/// GUI outline the source).
///
/// Returned in a list sorted by (priority, then distance to the query) so the
/// GUI treats index 0 as the default pick and Tab cycles through the rest —
/// the cycle/override affordance decision 011 §2.2 requires (impossible if the
/// engine returned only the single winner, ui-spec §2.4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapCandidate {
    /// The snap point, page space (PDF default user space).
    pub point: Point,
    /// What kind of geometry it locks onto.
    pub kind: SnapKind,
    /// Index into [`PageObjects::objects`] of the object this candidate came
    /// from, or `None` for a source with no single owning object (a page-axis
    /// or grid candidate, or a segment–segment intersection between two
    /// different objects).
    pub source_object: Option<usize>,
}

/// The engine's per-query configuration — everything the fixed target list
/// does NOT hardcode.
///
/// Both callers (the 12.M2 dimension tools and the future 9c-min node-drag)
/// build one of these per query. [`Self::tolerance`] is the page-space catch
/// radius the GUI derived from a screen-pixel radius ÷ zoom (module docs'
/// zoom-invariance). The two optional target categories default to the
/// cheap-and-safe choice: intersections OFF (Z4), grid absent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapConfig {
    /// Page-space catch radius. A candidate is kept only if its point is
    /// within this distance of the query. Non-finite/negative tolerances
    /// yield an empty result (no snapping), never a panic.
    pub tolerance: f64,
    /// Enable segment–segment intersection snapping (module docs: OFF by
    /// default, neighbourhood-bounded when on — Z4).
    pub intersections: bool,
    /// Optional grid spacing in page units. `Some(spacing)` adds nearest-grid-
    /// intersection snapping (as an [`SnapKind::Axis`] candidate); `None`
    /// (default) means no grid.
    pub grid: Option<f64>,
    /// Enable page-coordinate-axis snapping (`x = 0` / `y = 0`). Default on —
    /// it is the cheapest possible target and the lowest priority, so it never
    /// masks a real geometry hit.
    pub axes: bool,
}

impl SnapConfig {
    /// A config with the given page-space `tolerance` and the safe defaults:
    /// intersections OFF (Z4), no grid, page-axes ON.
    #[must_use]
    pub fn new(tolerance: f64) -> Self {
        Self {
            tolerance,
            intersections: false,
            grid: None,
            axes: true,
        }
    }

    /// Builder: turn intersection snapping on (module docs' Z4 warning
    /// applies — the caller opts in deliberately).
    #[must_use]
    pub fn with_intersections(mut self, on: bool) -> Self {
        self.intersections = on;
        self
    }

    /// Builder: set an optional grid spacing (page units).
    #[must_use]
    pub fn with_grid(mut self, spacing: Option<f64>) -> Self {
        self.grid = spacing;
        self
    }

    /// Builder: enable/disable page-axis snapping.
    #[must_use]
    pub fn with_axes(mut self, on: bool) -> Self {
        self.axes = on;
        self
    }

    /// Whether the tolerance is a usable, positive, finite catch radius.
    fn tolerance_ok(self) -> bool {
        self.tolerance.is_finite() && self.tolerance > 0.0
    }
}

/// The linear-dimension alignment constraint — the operator's "snap to
/// horizontally or vertically aligned" option (decision 011 §2.2).
///
/// A measurement pick constrains the second point's relationship to the first:
/// [`Self::Horizontal`] projects onto the page **X** axis (the pair shares a
/// Y coordinate, measured length `|Δx|`); [`Self::Vertical`] onto the page
/// **Y** axis (shares X, length `|Δy|`); [`Self::Aligned`] is free Euclidean.
///
/// **Rotation-correct by construction:** the projection is expressed in **page
/// space** (PDF default user space), and the GUI feeds page-space points via
/// its rotation-correct `viewer::canvas_to_pdf_space` bridge (proven at
/// 0/90/180/270° in `pdfce-gui`'s `viewer` tests). So "page X axis" means the
/// document's own X axis regardless of the page's `/Rotate`, exactly as
/// decision 011 specifies — a screen-space implementation would wrongly follow
/// the visual axes under a rotated page; this one does not, because it never
/// sees screen space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisConstraint {
    /// Free direction — the measured length is the Euclidean distance.
    Aligned,
    /// Constrain to the page X axis — the second point shares the first's Y,
    /// measured length `|Δx|`.
    Horizontal,
    /// Constrain to the page Y axis — the second point shares the first's X,
    /// measured length `|Δy|`.
    Vertical,
}

/// Project `raw_second` under `constraint` relative to `first`, both in page
/// space (decision 011 §2.2). Under [`AxisConstraint::Horizontal`] the result
/// shares `first.y`; under [`AxisConstraint::Vertical`] it shares `first.x`;
/// under [`AxisConstraint::Aligned`] it is returned unchanged.
///
/// This is what makes the on-canvas preview line show *exactly* what will be
/// measured — the committed second point is the projected one, not the raw
/// diagonal (ui-spec §2.5). Pure and finite-in/finite-out (a non-finite input
/// coordinate is passed through the arithmetic unchanged; callers gate on
/// finiteness upstream).
///
/// # Examples
///
/// ```
/// use pdfcer_core::vector::{AxisConstraint, Point, constrained_second_point};
/// let a = Point::new(10.0, 20.0);
/// let b = Point::new(50.0, 80.0);
/// // Horizontal: share A's y, so the segment is purely along page X.
/// assert_eq!(
///     constrained_second_point(a, b, AxisConstraint::Horizontal),
///     Point::new(50.0, 20.0)
/// );
/// // Vertical: share A's x.
/// assert_eq!(
///     constrained_second_point(a, b, AxisConstraint::Vertical),
///     Point::new(10.0, 80.0)
/// );
/// ```
#[must_use]
pub fn constrained_second_point(
    first: Point,
    raw_second: Point,
    constraint: AxisConstraint,
) -> Point {
    match constraint {
        AxisConstraint::Aligned => raw_second,
        AxisConstraint::Horizontal => Point::new(raw_second.x, first.y),
        AxisConstraint::Vertical => Point::new(first.x, raw_second.y),
    }
}

/// The measured page-space length of a dimension from `first` to `second`
/// under `constraint` — the value a linear dimension records before scaling
/// (decision 011 §2.3, `measured_pdf_length`). [`AxisConstraint::Horizontal`]
/// = `|Δx|`, [`AxisConstraint::Vertical`] = `|Δy|`, [`AxisConstraint::Aligned`]
/// = Euclidean distance. Computed from the RAW second point (projecting first
/// then measuring gives the identical result, and this avoids a redundant
/// projection).
#[must_use]
pub fn measured_length(first: Point, second: Point, constraint: AxisConstraint) -> f64 {
    match constraint {
        AxisConstraint::Aligned => first.distance(second),
        AxisConstraint::Horizontal => (second.x - first.x).abs(),
        AxisConstraint::Vertical => (second.y - first.y).abs(),
    }
}

/// The total page-space length of a polyline through `points` — the value a
/// [`PERIMETER`](crate::dimension::DimensionKind::Perimeter) ce dimension
/// records before scaling (`Pass 107.0`). When `closed`, the segment from the
/// last vertex back to the first is included; that segment is the entire
/// difference between a perimeter and a path length.
///
/// # Why it lives here rather than on the enum
///
/// A shell needs the running total DURING the pick, before any ce dimension
/// exists to hold it — the operator is clicking around a footprint and the
/// number beside the cursor has to be the number the committed measurement
/// will print. One function, called from the preview and from
/// [`DimensionKind::measured_points`](crate::dimension::DimensionKind::measured_points),
/// is what makes those two the same number by construction rather than by two
/// implementations agreeing. Beside [`measured_length`] because it is the same
/// category of thing: a page-space measurement primitive with no opinion about
/// scale, units or annotations.
///
/// # No axis constraint, on purpose
///
/// [`measured_length`] takes an [`AxisConstraint`] because a linear dimension
/// can measure only the horizontal or vertical component of a span. A polyline
/// has no single axis, so summing projected segment lengths would produce a
/// number that is not the length of anything drawn — the exact disagreement
/// between a line and its own caption that `Pass 27.0` existed to fix.
///
/// Fewer than two points is `0.0`, not an error: a one-vertex polyline has no
/// segments, and a caller mid-pick legitimately holds one.
///
/// # Examples
///
/// ```
/// use pdfcer_core::vector::{polyline_length, Point};
///
/// let square = [
///     Point::new(0.0, 0.0),
///     Point::new(10.0, 0.0),
///     Point::new(10.0, 10.0),
///     Point::new(0.0, 10.0),
/// ];
/// // Open: three sides.
/// assert_eq!(polyline_length(&square, false), 30.0);
/// // Closed: the fourth side joins back to the start.
/// assert_eq!(polyline_length(&square, true), 40.0);
/// assert_eq!(polyline_length(&square[..1], true), 0.0);
/// ```
#[must_use]
pub fn polyline_length(points: &[Point], closed: bool) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    // `zip(skip(1))` rather than `windows(2)` indexing: the pairs are the same,
    // and this form has no index to be out of bounds, so it satisfies
    // `clippy::indexing_slicing` without an allow (ARCHITECTURE.md §10 —
    // panic-free by construction, not by inspection).
    let open: f64 = points
        .iter()
        .zip(points.iter().skip(1))
        .map(|(a, b)| a.distance(*b))
        .sum();
    if closed {
        // `last`/`first` are both present: the length check above guarantees
        // at least two vertices, so neither unwrap-shaped access can fail —
        // expressed with `map_or` so there is no panic path at all
        // (`docs/ARCHITECTURE.md` §10).
        open + points
            .last()
            .zip(points.first())
            .map_or(0.0, |(l, f)| l.distance(*f))
    } else {
        open
    }
}

/// Every snap candidate within `config.tolerance` of `query` (page space),
/// sorted by **(priority ascending, then distance ascending)** — decision 011
/// §2.2's fixed target list, ties broken by nearest.
///
/// Returning the FULL sorted list (not just the winner) is what makes the
/// GUI's Tab-cycle / override possible (ui-spec §2.4): index 0 is the default
/// pick, Tab advances through the rest, Alt suppresses the whole query. The
/// result is deterministic for a given `(query, config, model)` — the sort key
/// is total (priority, then `f64::total_cmp` distance, then coordinates, then
/// source index), so two runs never disagree on the order or the winner.
///
/// Serves both the Pass 12.M2 dimension tools and the future Pass 9c-min
/// node-drag unchanged: the query point is wherever the pointer (or a dragged
/// node) currently is; the caller decides what to do with the winner.
///
/// Panic-free (`docs/ARCHITECTURE.md` §10): a non-finite `query`, a bad
/// tolerance, or a hostile object model yields an empty list, never a panic —
/// every coordinate is finiteness-checked and the intersection scan is capped
/// ([`MAX_NEIGHBOURHOOD_SEGMENTS`], [`MAX_CANDIDATES`]).
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{
///     decompose, snap_candidates, Matrix, NoXObjects, Point, SnapConfig, SnapKind,
/// };
/// // A horizontal stroked line from (10,20) to (100,20).
/// let cs = ContentStream::parse(b"10 20 m 100 20 l S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// // A query near the left endpoint resolves it first (highest priority).
/// let cands = snap_candidates(Point::new(11.0, 21.0), &SnapConfig::new(5.0), &model);
/// assert_eq!(cands[0].kind, SnapKind::Endpoint);
/// assert_eq!(cands[0].point, Point::new(10.0, 20.0));
/// ```
#[must_use]
pub fn snap_candidates(
    query: Point,
    config: &SnapConfig,
    model: &PageObjects,
) -> Vec<SnapCandidate> {
    if !query.is_finite() || !config.tolerance_ok() {
        return Vec::new();
    }
    let tol = config.tolerance;
    let mut out: Vec<SnapCandidate> = Vec::new();

    // Pass 9a's derived filled-quad centerlines, computed once (cheap: one
    // pass over objects, only filled quads qualify). A future caller that
    // already holds the candidate set could pass it in; recomputing here keeps
    // the API a single (query, config, model) call.
    let centerlines = page_candidates(model);

    for (i, obj) in model.objects.iter().enumerate() {
        // Object-level bbox reject: skip an object whose page bbox, widened by
        // the tolerance, does not reach the query. (A path with no finite
        // geometry has an empty bbox and is skipped.)
        if !obj.page_bbox().inflate(tol).contains(query) {
            continue;
        }
        let VectorObject::Path(path) = obj else {
            // Text/image/form objects carry no snap-able node geometry in the
            // beta (decision 011 §2.1 — they are selectable-for-move/delete
            // only). Their bbox is not a snap target (that would be the
            // bbox-corner category, explicitly OUT — module docs).
            continue;
        };
        collect_path_targets(i, path, query, tol, &mut out);
    }

    // Derived centerlines (Pass 9a fuzzy inference) — projected onto the
    // midline, kept if within tolerance. A distinct kind so the GUI renders
    // it distinctly and gates it behind the two-click confirm (module docs).
    for c in &centerlines {
        push_if_near(
            &mut out,
            project_onto_segment(query, c.start, c.end),
            SnapKind::DerivedCenterline,
            Some(c.object_index),
            query,
            tol,
        );
    }

    // Intersections — OFF by default, neighbourhood-bounded (Z4, module docs).
    if config.intersections {
        collect_intersections(model, query, tol, &mut out);
    }

    // Page axes / optional grid — the lowest-priority fallback.
    if config.axes {
        collect_axis_targets(query, tol, &mut out);
    }
    if let Some(spacing) = config.grid {
        collect_grid_target(query, tol, spacing, &mut out);
    }

    finalize(out, query)
}

/// Enumerate a path object's node / endpoint / center / midpoint /
/// segment-centerline targets near `query`, appending those within `tol`.
fn collect_path_targets(
    obj_index: usize,
    path: &PathObject,
    query: Point,
    tol: f64,
    out: &mut Vec<SnapCandidate>,
) {
    // Circle/arc centre (priority 3): a closed, curve-dominated path's centre
    // (object-centre approximation — module docs on `SnapKind::Center`).
    if let Some(center) = circle_center(path) {
        push_if_near(out, center, SnapKind::Center, Some(obj_index), query, tol);
    }

    for sp in &path.page_subpaths() {
        collect_subpath_targets(obj_index, sp, query, tol, out);
    }
}

/// Node/endpoint/midpoint/segment-centerline targets of one page-space
/// subpath.
fn collect_subpath_targets(
    obj_index: usize,
    sp: &Subpath,
    query: Point,
    tol: f64,
    out: &mut Vec<SnapCandidate>,
) {
    let anchors: Vec<Point> = sp.anchors().filter(|p| p.is_finite()).collect();
    let n = anchors.len();

    // Nodes vs endpoints: an anchor is an ENDPOINT only if the subpath is
    // open AND it is a free terminus (the first or the last anchor); every
    // other anchor (interior, or any anchor of a closed subpath) is a NODE
    // (module docs on the two kinds).
    for (idx, &a) in anchors.iter().enumerate() {
        let is_terminus = idx == 0 || idx + 1 == n;
        let kind = if !sp.closed && is_terminus {
            SnapKind::Endpoint
        } else {
            SnapKind::Node
        };
        push_if_near(out, a, kind, Some(obj_index), query, tol);
    }

    // Straight-segment midpoints (priority 4) + on-segment projections
    // (priority 6) walk the ORIGINAL segments so a midpoint is the true
    // segment midpoint (not a flattened-chord midpoint) and a cubic is
    // flattened only for its projection.
    let mut from = sp.start;
    for seg in &sp.segments {
        match *seg {
            Segment::Line { to } => {
                if from.is_finite() && to.is_finite() {
                    push_if_near(
                        out,
                        from.midpoint(to),
                        SnapKind::Midpoint,
                        Some(obj_index),
                        query,
                        tol,
                    );
                    push_if_near(
                        out,
                        project_onto_segment(query, from, to),
                        SnapKind::SegmentCenterline,
                        Some(obj_index),
                        query,
                        tol,
                    );
                }
                from = to;
            }
            Segment::Cubic { c1, c2, to } => {
                // Flatten to chords for the on-segment projection only; a
                // cubic contributes no straight-segment midpoint (module docs).
                project_cubic(query, [from, c1, c2, to], obj_index, tol, out);
                from = to;
            }
        }
    }
    // A closed subpath's closing edge (last anchor → start) is a real straight
    // segment: it has a midpoint and is projectable.
    if sp.closed
        && let (Some(&last), true) = (anchors.last(), sp.start.is_finite())
        && last.is_finite()
    {
        push_if_near(
            out,
            last.midpoint(sp.start),
            SnapKind::Midpoint,
            Some(obj_index),
            query,
            tol,
        );
        push_if_near(
            out,
            project_onto_segment(query, last, sp.start),
            SnapKind::SegmentCenterline,
            Some(obj_index),
            query,
            tol,
        );
    }
}

/// Project the query onto a cubic `[p0, c1, c2, p3]` (flattened to
/// [`SNAP_FLATTEN_STEPS`] chords), appending a [`SnapKind::SegmentCenterline`]
/// candidate for the nearest chord within tolerance (the single best chord
/// projection, so a curve contributes one on-curve snap point near the query,
/// not one per chord).
fn project_cubic(
    query: Point,
    cubic: [Point; 4],
    obj_index: usize,
    tol: f64,
    out: &mut Vec<SnapCandidate>,
) {
    let [p0, c1, c2, p3] = cubic;
    if !(p0.is_finite() && c1.is_finite() && c2.is_finite() && p3.is_finite()) {
        return;
    }
    let mut best: Option<(Point, f64)> = None;
    let mut prev = p0;
    for step in 1..=SNAP_FLATTEN_STEPS {
        let t = step as f64 / SNAP_FLATTEN_STEPS as f64;
        let cur = cubic_at(p0, c1, c2, p3, t);
        let proj = project_onto_segment(query, prev, cur);
        let d = proj.distance(query);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((proj, d));
        }
        prev = cur;
    }
    if let Some((proj, d)) = best
        && d <= tol
    {
        out.push(SnapCandidate {
            point: proj,
            kind: SnapKind::SegmentCenterline,
            source_object: Some(obj_index),
        });
    }
}

/// A cubic Bézier point at parameter `t` (de Casteljau, closed form) — the
/// same evaluation [`super::hit`] flattens with.
fn cubic_at(p0: Point, c1: Point, c2: Point, p3: Point, t: f64) -> Point {
    let u = 1.0 - t;
    let w0 = u * u * u;
    let w1 = 3.0 * u * u * t;
    let w2 = 3.0 * u * t * t;
    let w3 = t * t * t;
    Point::new(
        w0 * p0.x + w1 * c1.x + w2 * c2.x + w3 * p3.x,
        w0 * p0.y + w1 * c1.y + w2 * c2.y + w3 * p3.y,
    )
}

/// The centre of a circle/ellipse-like path object, or `None` if it is not
/// circle-like enough to offer a centre (module docs on [`SnapKind::Center`]).
///
/// Heuristic (the object-centre approximation; Taubin best-fit is 12.M2):
/// exactly one subpath, **closed**, with at least three segments that are ALL
/// cubics (a PDF circle is four ≈0.5523-κ cubics; an ellipse likewise). A
/// rectangle (all `Line` segments) or an open contour is rejected. The centre
/// is the page-space bounding-box centre of the subpath's anchors, which is
/// the true centre for any centrally-symmetric shape (a circle or an ellipse,
/// rotated or not).
fn circle_center(path: &PathObject) -> Option<Point> {
    let [sp] = path.subpaths.as_slice() else {
        return None;
    };
    if !sp.closed || sp.segments.len() < 3 {
        return None;
    }
    if !sp
        .segments
        .iter()
        .all(|s| matches!(s, Segment::Cubic { .. }))
    {
        return None;
    }
    let page = sp.transformed(path.ctm);
    let mut b = Bounds::EMPTY;
    for a in page.anchors() {
        b = b.union_point(a);
    }
    if b.is_empty() {
        return None;
    }
    Some(b.min.midpoint(b.max))
}

/// Collect segment–segment intersections near `query`, neighbourhood-bounded
/// (module docs' Z4 mitigation). Gathers the straight segments (lines +
/// flattened cubic chords) whose bounding box is within `tol` of the query —
/// capped at [`MAX_NEIGHBOURHOOD_SEGMENTS`] — then tests each pair, appending
/// crossings within `tol` of the query.
fn collect_intersections(
    model: &PageObjects,
    query: Point,
    tol: f64,
    out: &mut Vec<SnapCandidate>,
) {
    // Index-free pairwise scan: peel the head, pair it with every later
    // segment, then advance to the tail (each unordered pair once). Bounded by
    // `near_query_segments`' cap (Z4), so this is not an unbounded all-pairs
    // scan over the page.
    let segs = near_query_segments(model, query, tol);
    let mut head = segs.as_slice();
    while let Some((&(sa, oa), tail)) = head.split_first() {
        for &(sb, ob) in tail {
            if let Some(p) = segment_intersection(sa.0, sa.1, sb.0, sb.1)
                && p.distance(query) <= tol
            {
                // The intersection belongs to two objects; if they are the
                // same object, name it, else leave the source unattributed.
                let source = if oa == ob { Some(oa) } else { None };
                out.push(SnapCandidate {
                    point: p,
                    kind: SnapKind::Intersection,
                    source_object: source,
                });
            }
        }
        head = tail;
    }
}

/// The straight segments (lines + flattened cubic chords, each with its
/// owning object index) whose bounding box reaches within `tol` of `query` —
/// the spatial pre-filter that keeps the intersection scan local (Z4). Capped
/// at [`MAX_NEIGHBOURHOOD_SEGMENTS`]; once the cap is hit, no more are gathered
/// (the scan runs on the first N near the query — a bounded, honest subset).
fn near_query_segments(
    model: &PageObjects,
    query: Point,
    tol: f64,
) -> Vec<((Point, Point), usize)> {
    let mut segs: Vec<((Point, Point), usize)> = Vec::new();
    'objects: for (i, obj) in model.objects.iter().enumerate() {
        let VectorObject::Path(path) = obj else {
            continue;
        };
        if !path.page_bbox.inflate(tol).contains(query) {
            continue;
        }
        for sp in &path.page_subpaths() {
            let mut from = sp.start;
            let push_seg = |a: Point, b: Point, segs: &mut Vec<((Point, Point), usize)>| -> bool {
                if a.is_finite() && b.is_finite() && seg_bbox(a, b).inflate(tol).contains(query) {
                    segs.push(((a, b), i));
                }
                segs.len() < MAX_NEIGHBOURHOOD_SEGMENTS
            };
            for seg in &sp.segments {
                match *seg {
                    Segment::Line { to } => {
                        if !push_seg(from, to, &mut segs) {
                            break 'objects;
                        }
                        from = to;
                    }
                    Segment::Cubic { c1, c2, to } => {
                        let mut prev = from;
                        for step in 1..=SNAP_FLATTEN_STEPS {
                            let t = step as f64 / SNAP_FLATTEN_STEPS as f64;
                            let cur = cubic_at(from, c1, c2, to, t);
                            if !push_seg(prev, cur, &mut segs) {
                                break 'objects;
                            }
                            prev = cur;
                        }
                        from = to;
                    }
                }
            }
            if sp.closed && !push_seg(from, sp.start, &mut segs) {
                break 'objects;
            }
        }
    }
    segs
}

/// The bounding box of a single segment.
fn seg_bbox(a: Point, b: Point) -> Bounds {
    Bounds::EMPTY.union_point(a).union_point(b)
}

/// Page-axis targets: snap to the `x = 0` line (keeping the query's Y) and/or
/// the `y = 0` line (keeping the query's X), each when the query is within
/// `tol` of that axis (module docs on [`SnapKind::Axis`]).
fn collect_axis_targets(query: Point, tol: f64, out: &mut Vec<SnapCandidate>) {
    if query.x.abs() <= tol {
        push_if_near(
            out,
            Point::new(0.0, query.y),
            SnapKind::Axis,
            None,
            query,
            tol,
        );
    }
    if query.y.abs() <= tol {
        push_if_near(
            out,
            Point::new(query.x, 0.0),
            SnapKind::Axis,
            None,
            query,
            tol,
        );
    }
}

/// Grid target: the nearest grid intersection (multiples of `spacing` on both
/// axes) to the query, kept if within `tol`. A non-positive/non-finite spacing
/// is ignored.
fn collect_grid_target(query: Point, tol: f64, spacing: f64, out: &mut Vec<SnapCandidate>) {
    if !(spacing.is_finite() && spacing > 0.0) {
        return;
    }
    let gx = (query.x / spacing).round() * spacing;
    let gy = (query.y / spacing).round() * spacing;
    push_if_near(out, Point::new(gx, gy), SnapKind::Axis, None, query, tol);
}

/// Append `(point, kind)` from `source` if `point` is finite and within `tol`
/// of `query`.
fn push_if_near(
    out: &mut Vec<SnapCandidate>,
    point: Point,
    kind: SnapKind,
    source: Option<usize>,
    query: Point,
    tol: f64,
) {
    if point.is_finite() && point.distance(query) <= tol {
        out.push(SnapCandidate {
            point,
            kind,
            source_object: source,
        });
    }
}

/// The point on segment `a`–`b` nearest to `p` (the perpendicular projection,
/// clamped to the segment ends). A degenerate segment (`a == b`) reduces to
/// `a`.
fn project_onto_segment(p: Point, a: Point, b: Point) -> Point {
    let vx = b.x - a.x;
    let vy = b.y - a.y;
    let len2 = vx * vx + vy * vy;
    if len2 <= 0.0 {
        return a;
    }
    let t = (((p.x - a.x) * vx + (p.y - a.y) * vy) / len2).clamp(0.0, 1.0);
    Point::new(a.x + t * vx, a.y + t * vy)
}

/// The intersection point of two segments `a0`–`a1` and `b0`–`b1`, or `None`
/// if they are parallel/collinear or do not cross within both segments'
/// extents. Standard parametric solve; a near-zero denominator (parallel) or a
/// non-finite operand returns `None` (no intersection, never a panic).
fn segment_intersection(a0: Point, a1: Point, b0: Point, b1: Point) -> Option<Point> {
    let r = (a1.x - a0.x, a1.y - a0.y);
    let s = (b1.x - b0.x, b1.y - b0.y);
    let denom = r.0 * s.1 - r.1 * s.0;
    if !denom.is_finite() || denom.abs() <= f64::EPSILON {
        return None; // parallel or collinear
    }
    let qp = (b0.x - a0.x, b0.y - a0.y);
    let t = (qp.0 * s.1 - qp.1 * s.0) / denom;
    let u = (qp.0 * r.1 - qp.1 * r.0) / denom;
    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        let p = Point::new(a0.x + t * r.0, a0.y + t * r.1);
        p.is_finite().then_some(p)
    } else {
        None
    }
}

/// Sort by (priority, distance to `query`), dedup coincident points keeping
/// the higher-priority/nearer one, and cap the list — the deterministic final
/// ordering the GUI cycles through (module docs on [`snap_candidates`]).
fn finalize(mut cands: Vec<SnapCandidate>, query: Point) -> Vec<SnapCandidate> {
    // Total, deterministic order: priority, then distance (total_cmp handles
    // any stray non-finite deterministically), then coordinates and source so
    // the order never depends on insertion order.
    cands.sort_by(|a, b| {
        a.kind
            .priority()
            .cmp(&b.kind.priority())
            .then_with(|| a.point.distance(query).total_cmp(&b.point.distance(query)))
            .then_with(|| a.point.x.total_cmp(&b.point.x))
            .then_with(|| a.point.y.total_cmp(&b.point.y))
            .then_with(|| a.source_object.cmp(&b.source_object))
    });
    cands.truncate(MAX_CANDIDATES);

    // Dedup coincident points (a shared vertex, a midpoint that lands on a
    // projection): the first occurrence — already the highest-priority /
    // nearest after the sort — is kept, later coincidents dropped.
    let mut seen: HashSet<(i64, i64)> = HashSet::new();
    cands.retain(|c| seen.insert(quantize(c.point)));
    cands
}

/// Quantize a page-space point to a [`DEDUP_QUANTUM`] grid cell key for
/// coincident-point dedup. A non-finite coordinate (already filtered upstream,
/// but defended here) maps to a fixed sentinel cell so it never panics on the
/// `as i64` cast.
fn quantize(p: Point) -> (i64, i64) {
    let q = |v: f64| -> i64 {
        if v.is_finite() {
            (v / DEDUP_QUANTUM).round() as i64
        } else {
            i64::MIN
        }
    };
    (q(p.x), q(p.y))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;
    use crate::content::ContentStream;
    use crate::vector::decompose::{NoXObjects, decompose};
    use crate::vector::geometry::Matrix;

    fn model(src: &[u8]) -> PageObjects {
        let cs = ContentStream::parse(src.to_vec()).unwrap();
        decompose(&cs, Matrix::IDENTITY, &NoXObjects)
    }

    fn kinds(cands: &[SnapCandidate]) -> Vec<SnapKind> {
        cands.iter().map(|c| c.kind).collect()
    }

    fn near(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-6
    }

    // ---- priority + nearest tie-breaking (deterministic) ----------------

    #[test]
    fn priority_ranks_follow_decision_011_high_to_low() {
        // The fixed order the whole engine sorts by.
        let order = [
            SnapKind::Node,
            SnapKind::Endpoint,
            SnapKind::Center,
            SnapKind::Midpoint,
            SnapKind::Intersection,
            SnapKind::DerivedCenterline,
            SnapKind::SegmentCenterline,
            SnapKind::Axis,
        ];
        for w in order.windows(2) {
            assert!(w[0].priority() < w[1].priority());
        }
        assert!(SnapKind::DerivedCenterline.is_derived());
        assert!(!SnapKind::SegmentCenterline.is_derived());
    }

    #[test]
    fn a_node_outranks_a_coincident_lower_priority_target() {
        // A closed triangle: the apex is a NODE. A query right at the apex
        // must resolve the node first, ahead of any midpoint/centerline that
        // is also nearby.
        let m = model(b"0 0 m 100 0 l 50 100 l h S");
        let cands = snap_candidates(Point::new(50.0, 100.0), &SnapConfig::new(6.0), &m);
        assert_eq!(cands[0].kind, SnapKind::Node);
        assert!(near(cands[0].point, Point::new(50.0, 100.0)));
    }

    #[test]
    fn among_equal_priority_the_nearest_wins() {
        // Two separate stroked lines, each contributing endpoints. A query
        // between them resolves the nearer endpoint first (both Endpoint).
        let m = model(b"0 0 m 0 40 l S 100 0 m 100 40 l S");
        let cands = snap_candidates(Point::new(8.0, 0.0), &SnapConfig::new(50.0), &m);
        let endpoints: Vec<_> = cands
            .iter()
            .filter(|c| c.kind == SnapKind::Endpoint)
            .collect();
        assert!(endpoints.len() >= 2);
        // The first endpoint is the (0,0) one, nearer to the query than (100,0).
        assert!(near(endpoints[0].point, Point::new(0.0, 0.0)));
    }

    #[test]
    fn the_query_result_is_deterministic() {
        let m = model(b"0 0 m 100 0 l 100 100 l 0 100 l h S");
        let a = snap_candidates(Point::new(50.0, 3.0), &SnapConfig::new(10.0), &m);
        let b = snap_candidates(Point::new(50.0, 3.0), &SnapConfig::new(10.0), &m);
        assert_eq!(a, b);
    }

    // ---- tolerance semantics (page space; zoom-invariance is the caller's) --

    #[test]
    fn a_target_just_outside_tolerance_is_excluded_just_inside_is_kept() {
        // Endpoint at (10,20). Query 4 units away is inside tol=5, outside tol=3.
        let m = model(b"10 20 m 100 20 l S");
        let q = Point::new(10.0, 24.0); // distance 4 from (10,20)
        assert!(
            snap_candidates(q, &SnapConfig::new(5.0), &m)
                .iter()
                .any(|c| c.kind == SnapKind::Endpoint)
        );
        assert!(
            !snap_candidates(q, &SnapConfig::new(3.0), &m)
                .iter()
                .any(|c| c.kind == SnapKind::Endpoint)
        );
    }

    #[test]
    fn a_bad_query_or_tolerance_yields_nothing_not_a_panic() {
        let m = model(b"10 20 m 100 20 l S");
        assert!(snap_candidates(Point::new(f64::NAN, 0.0), &SnapConfig::new(5.0), &m).is_empty());
        assert!(snap_candidates(Point::new(10.0, 20.0), &SnapConfig::new(f64::NAN), &m).is_empty());
        assert!(snap_candidates(Point::new(10.0, 20.0), &SnapConfig::new(-1.0), &m).is_empty());
        assert!(snap_candidates(Point::new(10.0, 20.0), &SnapConfig::new(0.0), &m).is_empty());
    }

    // ---- each target type is found on a fixture -------------------------

    #[test]
    fn endpoint_and_node_are_classified_by_open_vs_closed() {
        // Open polyline: the two ends are ENDPOINTS, the middle vertex a NODE.
        let m = model(b"0 0 m 50 0 l 100 0 l S");
        let ends = snap_candidates(Point::new(0.0, 0.0), &SnapConfig::new(3.0), &m);
        assert_eq!(ends[0].kind, SnapKind::Endpoint);
        let mid = snap_candidates(Point::new(50.0, 0.0), &SnapConfig::new(3.0), &m);
        // The (50,0) anchor is an interior node (the NODE wins by priority).
        assert_eq!(mid[0].kind, SnapKind::Node);
    }

    #[test]
    fn midpoint_of_a_straight_segment_is_found() {
        let m = model(b"0 0 m 100 0 l S");
        // Query just off the midpoint (50,0). The nearest anchor is 50 away
        // (outside a tol of 6), so the midpoint is the winner near (50,0).
        let cands = snap_candidates(Point::new(50.0, 3.0), &SnapConfig::new(6.0), &m);
        assert_eq!(cands[0].kind, SnapKind::Midpoint);
        assert!(near(cands[0].point, Point::new(50.0, 0.0)));
    }

    #[test]
    fn on_segment_centerline_projection_is_found() {
        let m = model(b"0 0 m 100 0 l S");
        // A query at (30,4) is nearest the on-segment point (30,0); no anchor
        // or midpoint is within tol=6 of it, so SegmentCenterline wins.
        let cands = snap_candidates(Point::new(30.0, 4.0), &SnapConfig::new(6.0), &m);
        assert_eq!(cands[0].kind, SnapKind::SegmentCenterline);
        assert!(near(cands[0].point, Point::new(30.0, 0.0)));
    }

    #[test]
    fn a_circle_object_offers_its_center() {
        // A circle centred at (50,50), radius 20, as four kappa Béziers.
        // 20 * 0.5523 = 11.046.
        let src = b"70 50 m \
                    70 61.046 61.046 70 50 70 c \
                    38.954 70 30 61.046 30 50 c \
                    30 38.954 38.954 30 50 30 c \
                    61.046 30 70 38.954 70 50 c h S";
        let m = model(src);
        let cands = snap_candidates(Point::new(51.0, 49.0), &SnapConfig::new(5.0), &m);
        assert!(
            cands
                .iter()
                .any(|c| c.kind == SnapKind::Center && near(c.point, Point::new(50.0, 50.0))),
            "expected a center candidate at (50,50), got {cands:?}"
        );
    }

    #[test]
    fn a_rectangle_offers_no_center() {
        // A closed all-line quad is not circle-like — no center candidate.
        let m = model(b"0 0 100 100 re S");
        let cands = snap_candidates(Point::new(50.0, 50.0), &SnapConfig::new(60.0), &m);
        assert!(cands.iter().all(|c| c.kind != SnapKind::Center));
    }

    #[test]
    fn a_derived_centerline_is_a_distinct_kind() {
        // A thin filled bar (aspect 50) → a derived-centerline candidate along
        // its midline y=102. A query near the midline offers DerivedCenterline.
        let m = model(b"10 100 200 4 re f");
        let cands = snap_candidates(Point::new(105.0, 103.0), &SnapConfig::new(4.0), &m);
        assert!(
            cands.iter().any(|c| c.kind == SnapKind::DerivedCenterline),
            "expected a derived-centerline candidate, got {cands:?}"
        );
        // It must be flagged as a fuzzy inference (drives the GUI confirm).
        let dc = cands
            .iter()
            .find(|c| c.kind == SnapKind::DerivedCenterline)
            .unwrap();
        assert!(dc.kind.is_derived());
        assert!(near(dc.point, Point::new(105.0, 102.0)));
    }

    #[test]
    fn a_page_axis_is_found_when_the_query_is_near_it() {
        // No geometry needed; the page X axis (x=0) is a target.
        let m = model(b"");
        let cands = snap_candidates(Point::new(2.0, 50.0), &SnapConfig::new(5.0), &m);
        assert!(
            cands
                .iter()
                .any(|c| c.kind == SnapKind::Axis && near(c.point, Point::new(0.0, 50.0))),
            "expected an axis candidate at (0,50), got {cands:?}"
        );
        // Far from both axes → no axis candidate.
        let far = snap_candidates(Point::new(500.0, 500.0), &SnapConfig::new(5.0), &m);
        assert!(far.iter().all(|c| c.kind != SnapKind::Axis));
    }

    #[test]
    fn a_grid_intersection_is_found_when_configured() {
        let m = model(b"");
        let cfg = SnapConfig::new(6.0).with_grid(Some(10.0)).with_axes(false);
        // Query (23,47) → nearest grid intersection (20,50), distance 5 < 6.
        let cands = snap_candidates(Point::new(23.0, 47.0), &cfg, &m);
        assert!(
            cands
                .iter()
                .any(|c| c.kind == SnapKind::Axis && near(c.point, Point::new(20.0, 50.0))),
            "expected a grid candidate at (20,50), got {cands:?}"
        );
    }

    // ---- intersection: OFF by default + neighbourhood-bounded (Z4) ------

    #[test]
    fn intersection_is_off_by_default() {
        // Two crossing lines at (50,50). Default config has intersections OFF.
        let m = model(b"0 0 m 100 100 l S 0 100 m 100 0 l S");
        let cands = snap_candidates(Point::new(50.0, 50.0), &SnapConfig::new(6.0), &m);
        assert!(
            cands.iter().all(|c| c.kind != SnapKind::Intersection),
            "intersection snapping must default off (Z4)"
        );
    }

    #[test]
    fn intersection_is_found_only_when_enabled_and_near_the_query() {
        // A horizontal line (y=0) and a vertical line (x=30) crossing at
        // (30,0). The crossing is deliberately NOT a midpoint or anchor of
        // either line (horizontal midpoint is (50,0); vertical midpoint is
        // (30,5)), so no higher-priority target sits on the crossing to mask
        // it via the coincident-point dedup — the intersection is the winner
        // there. (A crossing that DOES coincide with a node/midpoint is
        // correctly out-ranked by it; that is the dedup working, not a miss.)
        let m = model(b"0 0 m 100 0 l S 30 -20 m 30 30 l S");
        let cfg = SnapConfig::new(6.0).with_intersections(true);
        // Near the crossing (30,0): found.
        let near_cross = snap_candidates(Point::new(31.0, 1.0), &cfg, &m);
        assert!(
            near_cross
                .iter()
                .any(|c| c.kind == SnapKind::Intersection && near(c.point, Point::new(30.0, 0.0))),
            "expected the intersection at (30,0), got {near_cross:?}"
        );
        // Far from the crossing (10,25): the crossing point is > tol away, so
        // it is not returned (correctness), and the neighbourhood pre-filter
        // rejects both segments' bounding boxes before any pair is tested
        // (cost — the Z4 mitigation).
        let far = snap_candidates(Point::new(10.0, 25.0), &cfg, &m);
        assert!(far.iter().all(|c| c.kind != SnapKind::Intersection));
    }

    // ---- H/V constraint, correct under page rotation --------------------

    #[test]
    fn hv_constraint_projects_to_page_axes() {
        let a = Point::new(10.0, 20.0);
        let b = Point::new(50.0, 80.0);
        let h = constrained_second_point(a, b, AxisConstraint::Horizontal);
        assert_eq!(h, Point::new(50.0, 20.0)); // shares A.y (page X axis)
        assert_eq!(measured_length(a, b, AxisConstraint::Horizontal), 40.0); // |Δx|
        let v = constrained_second_point(a, b, AxisConstraint::Vertical);
        assert_eq!(v, Point::new(10.0, 80.0)); // shares A.x (page Y axis)
        assert_eq!(measured_length(a, b, AxisConstraint::Vertical), 60.0); // |Δy|
        // Aligned is free Euclidean.
        assert_eq!(constrained_second_point(a, b, AxisConstraint::Aligned), b);
        assert!(
            (measured_length(a, b, AxisConstraint::Aligned) - 40.0_f64.hypot(60.0)).abs() < 1e-9
        );
    }

    #[test]
    fn hv_projection_is_correct_under_page_rotation() {
        // Decision 011: "correct under page rotation." The engine works in
        // page space (PDF user space); the GUI's rotation-correct bridge feeds
        // page-space points regardless of the page's /Rotate (proven at
        // 0/90/180/270 in pdfce-gui's viewer tests). So the projection is
        // rotation-invariant: a physical pair of picks maps to the SAME
        // page-space coordinates at every rotation, hence the SAME constrained
        // result and the SAME measured length.
        //
        // We demonstrate this here by round-tripping the page-space picks
        // through each of the four /Rotate transforms and back (the GUI
        // bridge's job), then applying the constraint: the measured length is
        // identical at every rotation, and Horizontal always projects onto the
        // page X axis (shared Y).
        let a = Point::new(10.0, 20.0);
        let b = Point::new(50.0, 80.0);
        // (rotation, inverse-rotation) matrices about the origin.
        let pairs = [
            (Matrix::IDENTITY, Matrix::IDENTITY),
            (
                Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0),
                Matrix::new(0.0, -1.0, 1.0, 0.0, 0.0, 0.0),
            ), // 90
            (
                Matrix::new(-1.0, 0.0, 0.0, -1.0, 0.0, 0.0),
                Matrix::new(-1.0, 0.0, 0.0, -1.0, 0.0, 0.0),
            ), // 180
            (
                Matrix::new(0.0, -1.0, 1.0, 0.0, 0.0, 0.0),
                Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0),
            ), // 270
        ];
        for (rot, inv) in pairs {
            // The GUI maps a physical pick to display space (rot) then, via its
            // canvas->pdf bridge, back to page space (inv). Round-trip = the
            // same page-space points, at every rotation.
            let a_page = inv.map_point(rot.map_point(a));
            let b_page = inv.map_point(rot.map_point(b));
            let h = constrained_second_point(a_page, b_page, AxisConstraint::Horizontal);
            assert!((h.y - a_page.y).abs() < 1e-9, "horizontal shares page Y");
            assert!(
                (measured_length(a_page, b_page, AxisConstraint::Horizontal) - 40.0).abs() < 1e-9
            );
            assert!(
                (measured_length(a_page, b_page, AxisConstraint::Vertical) - 60.0).abs() < 1e-9
            );
        }
    }

    // ---- dedup + geometry helpers ---------------------------------------

    #[test]
    fn coincident_targets_dedup_keeping_the_higher_priority() {
        // Two open lines sharing the endpoint (0,0): both would emit an
        // Endpoint there; dedup keeps a single candidate at that point.
        let m = model(b"0 0 m 50 0 l S 0 0 m 0 50 l S");
        let cands = snap_candidates(Point::new(0.0, 0.0), &SnapConfig::new(3.0), &m);
        let at_origin: Vec<_> = cands
            .iter()
            .filter(|c| near(c.point, Point::new(0.0, 0.0)))
            .collect();
        assert_eq!(at_origin.len(), 1, "coincident targets collapse to one");
    }

    #[test]
    fn projection_clamps_to_the_segment_ends() {
        // A query beyond a segment's end projects to the nearer end.
        let a = Point::new(0.0, 0.0);
        let b = Point::new(10.0, 0.0);
        assert!(near(project_onto_segment(Point::new(-5.0, 0.0), a, b), a));
        assert!(near(project_onto_segment(Point::new(15.0, 0.0), a, b), b));
        assert!(near(
            project_onto_segment(Point::new(3.0, 4.0), a, b),
            Point::new(3.0, 0.0)
        ));
    }

    #[test]
    fn parallel_segments_do_not_intersect() {
        let a0 = Point::new(0.0, 0.0);
        let a1 = Point::new(10.0, 0.0);
        let b0 = Point::new(0.0, 5.0);
        let b1 = Point::new(10.0, 5.0);
        assert!(segment_intersection(a0, a1, b0, b1).is_none());
    }

    #[test]
    fn crossing_segments_intersect_at_the_crossing() {
        let p = segment_intersection(
            Point::new(0.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
            Point::new(10.0, 0.0),
        )
        .unwrap();
        assert!(near(p, Point::new(5.0, 5.0)));
    }

    #[test]
    fn a_page_with_no_vector_geometry_still_answers_axis_queries() {
        // A text-only page: no path targets, but the axis fallback works and
        // nothing panics.
        let m = model(b"BT /F1 12 Tf 72 700 Td (Hi) Tj ET");
        let cands = snap_candidates(Point::new(1.0, 700.0), &SnapConfig::new(5.0), &m);
        assert!(cands.iter().any(|c| c.kind == SnapKind::Axis));
        assert!(kinds(&cands).iter().all(|k| *k != SnapKind::Node));
    }
}

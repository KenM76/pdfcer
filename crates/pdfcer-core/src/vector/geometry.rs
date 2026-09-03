//! # Vector geometry primitives (ISO 32000-1 §8.3, §8.5)
//!
//! The hand-rolled 2-D affine geometry the read-only vector object model
//! (`docs/decisions/011-first-beta-scaled-measurement-dimensioning-tool.md`
//! §2.1) is built on: a [`Point`], a PDF-convention affine [`Matrix`], an
//! axis-aligned [`Bounds`], a working [`Rgb`] colour, and the **shared
//! path-construction primitives** ([`cubic_from_v`], [`cubic_from_y`],
//! [`rect_corners`]) that `pdfcer-render`'s interpreter also calls, so the
//! trap-prone operand arithmetic (`v`/`y`'s implicit control points,
//! `re`'s corner expansion) exists as ONE implementation rather than two
//! (the geometry analogue of the R49/R60 "one pipeline" discipline; the
//! Z2 risk mitigation in decision 011 Appendix A Pass 9a).
//!
//! ## Why hand-rolled and not a dependency (rule 13)
//!
//! pdfcer carries a ZERO-new-dependency posture through this Pass. An
//! affine 2×3 matrix, a point, and a bounding box are ~200 lines of
//! arithmetic; a linear-algebra crate would be a copyleft/licensing
//! classification and a WASM-fork weight for no benefit. `pdfcer-render`
//! rasterizes with `tiny-skia::Transform`, but `pdfcer-core` must stay
//! free of `tiny-skia` (it is GUI-adjacent render weight the WASM engine
//! fork should not inherit), so the core object model has its own matrix.
//! The two are kept in agreement **by construction**: the render walk
//! calls the shared primitives here for the exact same node values, and
//! an acceptance cross-check (in `pdfcer-render`'s tests) compares the full
//! page-space geometry the two produce on the fixtures.
//!
//! ## The PDF coordinate convention (row vectors, §8.3.3–§8.3.4)
//!
//! PDF transforms a point by **left-multiplication of a row vector**:
//! `[x' y' 1] = [x y 1] × M`, where
//!
//! ```text
//!       | a b 0 |
//!   M = | c d 0 |     x' = a·x + c·y + e     y' = b·x + d·y + f
//!       | e f 1 |
//! ```
//!
//! so `Matrix` stores exactly the six numbers a PDF `cm`/`Tm`/`/Matrix`
//! operand carries, in that order, and [`Matrix::map_point`] applies them
//! with that formula. Composition is the row-vector product: applying `A`
//! then `B` to a point is `p × A × B = p × (A·B)`, which is what
//! [`Matrix::post_concat`] computes (`A.post_concat(B)` = "apply A then
//! B"), matching `tiny-skia`'s `post_concat` and the render interpreter's
//! `m.post_concat(ctm)` for the `cm` operator.

/// A point in some 2-D coordinate space (PDF user space, or a page-space
/// image of it under a CTM). Values are `f64` — content-stream operands
/// are real numbers and the object model keeps full precision through the
/// transform chain, narrowing to `f32` only at the render/GUI boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate (PDF user space is Y-up).
    pub y: f64,
}

impl Point {
    /// A point from its two coordinates.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Euclidean distance to `other`. Uses [`f64::hypot`], which is
    /// overflow-robust for the coordinate magnitudes a PDF can carry.
    #[must_use]
    pub fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    /// The midpoint of the segment `self`–`other`.
    #[must_use]
    pub fn midpoint(self, other: Self) -> Self {
        Self::new((self.x + other.x) / 2.0, (self.y + other.y) / 2.0)
    }

    /// Whether both coordinates are finite (neither `NaN` nor infinite).
    ///
    /// The decomposition tolerates the degenerate/hostile operands a
    /// fuzzed content stream produces (`1e308 1e308 m`, `NaN` via a
    /// malformed real): a non-finite point is kept in the node list for
    /// lossless provenance but skipped by hit-testing and centerline
    /// math, which is what this predicate gates.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// A 2×3 affine transform in PDF row-vector convention (module docs).
///
/// The six fields are the `cm`/`Tm`/`/Matrix` operand order `a b c d e f`.
/// Deliberately `Copy` (48 bytes) so it threads through the graphics-state
/// stack and the per-object capture without allocation ceremony.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    /// Row-vector coefficient a (x-scale / cos component).
    pub a: f64,
    /// Row-vector coefficient b (y-shear / sin component).
    pub b: f64,
    /// Row-vector coefficient c (x-shear / −sin component).
    pub c: f64,
    /// Row-vector coefficient d (y-scale / cos component).
    pub d: f64,
    /// Row-vector translation e (Δx).
    pub e: f64,
    /// Row-vector translation f (Δy).
    pub f: f64,
}

impl Matrix {
    /// The identity transform — the initial CTM the page content stream is
    /// decomposed under, so an object's page-space geometry is genuine PDF
    /// default user space (§8.3.2.3).
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// A matrix from its six row-vector coefficients (the operand order of
    /// `cm`, `Tm`, and a `/Matrix` array).
    #[must_use]
    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Self { a, b, c, d, e, f }
    }

    /// A pure translation `[1 0 0 1 tx ty]` — the `Td`/`TD` text-line
    /// offset and the building block of the text-matrix walk (§9.4.2).
    #[must_use]
    pub const fn translate(tx: f64, ty: f64) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: tx,
            f: ty,
        }
    }

    /// A pure scale `[sx 0 0 sy 0 0]`, about the ORIGIN (`Pass 112.0`).
    ///
    /// # About the origin, deliberately — see [`Self::about`]
    ///
    /// A resize gesture is almost never about the origin; it is about the grip
    /// opposite the one being dragged. That pivot is the **shell's** to choose
    /// (the consuming shell asked, in as many words, that pdfcer not decide
    /// where the pivot is), so this constructor stays primitive and
    /// [`Self::about`] composes it with the operator's chosen point.
    ///
    /// A zero or non-finite factor is **not** rejected here: this is a value
    /// constructor with no operator context, and the verb that consumes a
    /// matrix is where a degenerate transform earns a *named* refusal the
    /// shell can act on. Silently clamping here would hide the drag-through-
    /// zero case the shell explicitly wants to distinguish.
    #[must_use]
    pub const fn scale(sx: f64, sy: f64) -> Self {
        Self {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            e: 0.0,
            f: 0.0,
        }
    }

    /// A pure rotation by `radians` **counter-clockwise**, about the origin
    /// (`Pass 112.0`).
    ///
    /// `[cos θ, sin θ, −sin θ, cos θ, 0, 0]` — the row-vector form, which is
    /// the one PDF's `cm` operand order wants. Getting the sign of `c` wrong
    /// produces a mirror image rather than an error, so the direction is
    /// pinned by a doc-test below.
    ///
    /// # Radians, not degrees
    ///
    /// Every trigonometric call in this crate takes radians and the one place
    /// degrees appear is a formatted ce-dimension label
    /// (`DimensionKind::measured_points`, which returns degrees precisely so a
    /// caller cannot feed an angle to a length formatter). A `rotate_degrees`
    /// convenience beside this would be a second spelling of one operation and
    /// the first place a caller passes 90.0 to the radians one.
    #[must_use]
    pub fn rotate(radians: f64) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: 0.0,
            f: 0.0,
        }
    }

    /// `self`, applied **about the point `pivot`** rather than the origin
    /// (`Pass 112.0`).
    ///
    /// `translate(−p) × self × translate(+p)` — move the pivot to the origin,
    /// transform, move it back. This is the form every direct-manipulation
    /// gesture actually wants: a resize grip pivots on the opposite corner, a
    /// rotation handle pivots on the selection centre, and neither is the page
    /// origin.
    ///
    /// # Why this exists rather than leaving the shell to compose it
    ///
    /// It is three multiplications in the right order, and *the order is the
    /// whole difficulty* — `post_concat`'s argument order is the reverse of
    /// the reading order in `translate(−p) × self × translate(+p)`, which is
    /// exactly the sort of thing that produces a shape that drifts a little
    /// on every drag and looks like a rounding bug. One implementation, one
    /// doc-test, and no consumer has to get it right twice.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::vector::{Matrix, Point};
    ///
    /// let pivot = Point::new(10.0, 10.0);
    /// // A half-turn about (10, 10) maps (11, 10) to (9, 10).
    /// let m = Matrix::rotate(std::f64::consts::PI).about(pivot);
    /// let p = m.map_point(Point::new(11.0, 10.0));
    /// assert!((p.x - 9.0).abs() < 1e-9, "got {}", p.x);
    /// assert!((p.y - 10.0).abs() < 1e-9, "got {}", p.y);
    ///
    /// // The pivot itself is always fixed — the defining property.
    /// let q = m.map_point(pivot);
    /// assert!((q.x - pivot.x).abs() < 1e-9 && (q.y - pivot.y).abs() < 1e-9);
    ///
    /// // A quarter-turn CCW about the origin takes +x to +y.
    /// let r = Matrix::rotate(std::f64::consts::FRAC_PI_2).map_point(Point::new(1.0, 0.0));
    /// assert!((r.x - 0.0).abs() < 1e-9 && (r.y - 1.0).abs() < 1e-9, "got {r:?}");
    ///
    /// // Scaling about a pivot leaves the pivot put.
    /// let s = Matrix::scale(3.0, 3.0).about(pivot).map_point(pivot);
    /// assert!((s.x - 10.0).abs() < 1e-9 && (s.y - 10.0).abs() < 1e-9);
    /// ```
    #[must_use]
    pub fn about(self, pivot: Point) -> Self {
        Self::translate(-pivot.x, -pivot.y)
            .post_concat(self)
            .post_concat(Self::translate(pivot.x, pivot.y))
    }

    /// Whether this matrix can be inverted — equivalently, whether it maps
    /// area to non-zero area (`Pass 112.0`).
    ///
    /// The predicate behind a *named* refusal for the case the consuming shell
    /// singled out: **an operator dragging a resize grip through zero**, which
    /// they will, and which must be distinguishable from "this object cannot
    /// be transformed at all" because the two produce different UI. Asking
    /// this is what lets a caller say which one happened without duplicating
    /// [`Self::inverse`]'s arithmetic to find out.
    ///
    /// Non-finite coefficients answer `false`: a matrix that cannot be
    /// written as six real numbers has no inverse worth claiming, and letting
    /// a `NaN` through here would put one in a `cm` operand where no reader
    /// can draw it.
    #[must_use]
    pub fn is_invertible(self) -> bool {
        let det = self.determinant();
        det.is_finite()
            && det != 0.0
            && [self.a, self.b, self.c, self.d, self.e, self.f]
                .iter()
                .all(|v| v.is_finite())
    }

    /// Transform `p` by this matrix: `p' = p × M` (module docs' formula).
    #[must_use]
    pub fn map_point(self, p: Point) -> Point {
        Point::new(
            self.a * p.x + self.c * p.y + self.e,
            self.b * p.x + self.d * p.y + self.f,
        )
    }

    /// The composition "apply `self`, then `other`" — the row-vector
    /// product `self · other`, so `self.post_concat(other).map_point(p)`
    /// equals `other.map_point(self.map_point(p))`.
    ///
    /// This is the operation the `cm` operator performs on the CTM
    /// (`CTM′ = M · CTM`, §8.3.4) and it is named and oriented to match
    /// `tiny-skia::Transform::post_concat` so the render interpreter's
    /// `m.post_concat(ctm)` and this object model's CTM update are the same
    /// composition — the agree-by-construction requirement for the CTM
    /// itself.
    #[must_use]
    pub fn post_concat(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    /// The signed area scale factor (`a·d − b·c`) — used to sanity-flag a
    /// degenerate (non-invertible) CTM and to estimate a coarse glyph
    /// scale for a text object's approximate bounds.
    #[must_use]
    pub fn determinant(self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// Transform a **direction/displacement** `v` by this matrix's linear
    /// part only (the translation `e`/`f` is deliberately ignored): `v' =
    /// v × L` where `L = [[a b][c d]]`, so `v'.x = a·v.x + c·v.y` and
    /// `v'.y = b·v.x + d·v.y`.
    ///
    /// This is the correct transform for a **delta** (the difference of two
    /// points): translating both endpoints of a segment by the same
    /// world-space offset moves the segment by that offset's *linear* image,
    /// with no additional shift from `e`/`f`. The 9c-min move operation
    /// (decision 011 §2.5) needs exactly this: the operator's drag is a
    /// page-space displacement, and the object's construction operands live
    /// in user space, so the user-space displacement is
    /// `ctm.inverse()?.map_vector(page_delta)` — the linear inverse image of
    /// the page-space drag, never the full affine image (which would fold in
    /// the CTM's translation and shove the object across the page).
    #[must_use]
    pub fn map_vector(self, v: Point) -> Point {
        Point::new(self.a * v.x + self.c * v.y, self.b * v.x + self.d * v.y)
    }

    /// The affine inverse `M⁻¹`, or `None` when `M` is singular
    /// (non-invertible) — a zero, non-finite, or numerically degenerate
    /// determinant.
    ///
    /// The inverse of a PDF row-vector affine `p' = p·L + t` (with linear
    /// part `L = [[a b][c d]]` and translation `t = (e, f)`) is
    /// `p = (p' − t)·L⁻¹ = p'·L⁻¹ − t·L⁻¹`, so the inverse's linear part is
    /// `L⁻¹ = (1/det)·[[d −b][−c a]]` and its translation is `−t·L⁻¹`. This
    /// is what maps a **page-space** point/drag back into an object's
    /// **user space** for the 9c-min move/drag-node surgery (decision 011
    /// §2.5): the object's construction operands must be rewritten in the
    /// user space the CTM maps *from*.
    ///
    /// Returns `None` rather than panicking on a singular CTM (the
    /// crate-wide panic-free policy, ARCHITECTURE.md §10); the caller
    /// surfaces it as a named refusal (`VectorEditError::DegenerateCtm`)
    /// instead of fabricating geometry — an object drawn under a
    /// rank-deficient CTM (scaled flat to a line) has no unambiguous
    /// user-space pre-image for a page-space drag.
    #[must_use]
    pub fn inverse(self) -> Option<Self> {
        let det = self.determinant();
        if !det.is_finite() || det == 0.0 {
            return None;
        }
        let a = self.d / det;
        let b = -self.b / det;
        let c = -self.c / det;
        let d = self.a / det;
        // Inverse translation = −t·L⁻¹, expressed in this matrix's own
        // `map_point` convention (x = a·x' + c·y' + e).
        let e = -(self.e * a + self.f * c);
        let f = -(self.e * b + self.f * d);
        let out = Self { a, b, c, d, e, f };
        // Guard against a determinant so small the quotients overflowed to
        // non-finite: an unusable inverse is `None`, not a silent NaN.
        if [out.a, out.b, out.c, out.d, out.e, out.f]
            .iter()
            .all(|v| v.is_finite())
        {
            Some(out)
        } else {
            None
        }
    }
}

/// An axis-aligned bounding box in one coordinate space, or the empty box.
///
/// Stored as min/max corners; the empty box is `min > max` on both axes
/// and is what an object with no finite geometry yields. Kept `Copy` for
/// the same threading reasons as [`Matrix`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// Lower-left corner (smaller x, smaller y).
    pub min: Point,
    /// Upper-right corner (larger x, larger y).
    pub max: Point,
}

impl Bounds {
    /// The empty box — `min = +∞`, `max = −∞` — so [`Bounds::union_point`]
    /// of the empty box with any finite point yields a degenerate box AT
    /// that point, and unioning two empty boxes stays empty. This is the
    /// standard "grow from nothing" accumulator seed.
    pub const EMPTY: Self = Self {
        min: Point {
            x: f64::INFINITY,
            y: f64::INFINITY,
        },
        max: Point {
            x: f64::NEG_INFINITY,
            y: f64::NEG_INFINITY,
        },
    };

    /// Whether this box encloses no area — the accumulator seed, or a box
    /// that never saw a finite point.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }

    /// Grow the box to include `p`; a non-finite `p` is ignored (it would
    /// poison the box with `NaN`/∞, exactly the hostile-operand case the
    /// fuzz target drives).
    #[must_use]
    pub fn union_point(self, p: Point) -> Self {
        if !p.is_finite() {
            return self;
        }
        Self {
            min: Point::new(self.min.x.min(p.x), self.min.y.min(p.y)),
            max: Point::new(self.max.x.max(p.x), self.max.y.max(p.y)),
        }
    }

    /// The union of two boxes (either may be empty).
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        self.union_point(other.min).union_point(other.max)
    }

    /// Grow the box by `margin` on every side (a no-op on the empty box).
    /// Used to give a text object's origin-derived bounds a coarse glyph
    /// margin, and to widen a hit-test bbox pre-filter by the tolerance.
    #[must_use]
    pub fn inflate(self, margin: f64) -> Self {
        if self.is_empty() {
            return self;
        }
        Self {
            min: Point::new(self.min.x - margin, self.min.y - margin),
            max: Point::new(self.max.x + margin, self.max.y + margin),
        }
    }

    /// Whether `p` lies within the closed box.
    #[must_use]
    pub fn contains(self, p: Point) -> bool {
        !self.is_empty()
            && p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
    }

    /// Whether this box lies wholly inside `outer` (the fully-contained
    /// marquee-enclosure test — decision 011's default, grounded in
    /// Inkscape's default rubber-band-selects-fully-enclosed behavior,
    /// R61).
    #[must_use]
    pub fn contained_by(self, outer: Self) -> bool {
        !self.is_empty()
            && !outer.is_empty()
            && self.min.x >= outer.min.x
            && self.min.y >= outer.min.y
            && self.max.x <= outer.max.x
            && self.max.y <= outer.max.y
    }

    /// Whether the two boxes overlap at all (the alternate, partial-
    /// enclosure marquee test, selectable via [`crate::vector::hit`]).
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }
}

/// A working RGB colour in `[0, 1]` components — the object model's record
/// of an object's paint colour at paint time (§8.6.4 device colours),
/// captured for display/inspection.
///
/// Every constructor delegates to [`crate::color`], which is also what
/// `pdfcer-render`'s graphics state calls, so a decomposed object's recorded
/// colour is the pixel the renderer paints **by construction** rather than by
/// two copies of a formula staying in sync. That mattered: until 2026-08-08
/// these were two hand-copied implementations of the naive additive CMYK
/// conversion, and calibrating one without the other would have made the
/// object model's reported colour disagree with the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    /// Red, 0.0–1.0.
    pub r: f32,
    /// Green, 0.0–1.0.
    pub g: f32,
    /// Blue, 0.0–1.0.
    pub b: f32,
}

impl Rgb {
    /// Black — the initial colour in every device space (§8.6.4).
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
    };

    /// Build from a `[r, g, b]` triple as [`crate::color`] returns one.
    const fn from_triple([r, g, b]: [f32; 3]) -> Self {
        Self { r, g, b }
    }

    /// From a DeviceGray value (`g`/`G`) — §8.6.4.2.
    #[must_use]
    pub fn from_gray(v: f32) -> Self {
        Self::from_triple(crate::color::gray_to_srgb(v))
    }

    /// From DeviceRGB components (`rg`/`RG`) — §8.6.4.3.
    #[must_use]
    pub fn from_rgb(r: f32, g: f32, b: f32) -> Self {
        Self::from_triple(crate::color::rgb_to_srgb(r, g, b))
    }

    /// From DeviceCMYK components (`k`/`K`) — §8.6.4.4, under the **default**
    /// [`CmykIntent`](crate::settings::CmykIntent).
    ///
    /// # ★ Why the default and not the raw table
    ///
    /// This called [`crate::color::cmyk_to_srgb`] directly, which is the
    /// **calibrated** table unconditionally — and the renderer does not. Every
    /// paint path goes through `pdfcer_render::gstate::Rgb::from_cmyk(intent,
    /// …)`, honouring the operator's chosen intent, whose shipped default is
    /// [`CmykIntent::NeutralBlack`](crate::settings::CmykIntent::NeutralBlack).
    ///
    /// The two disagree by up to **38/255** across the grey ramp, and worst
    /// exactly where this project's document population lives: pure-K line
    /// art. `0 0 0 1 K` decomposed to `#231F20` while the canvas painted it
    /// `#000000` — the operator ruling behind `NeutralBlack` is precisely
    /// that CAD line art must be true black, and this path was not honouring
    /// it. So "what colour is this line?" and "what colour is this line
    /// painted?" returned different answers for the commonest case in the
    /// corpus.
    ///
    /// # ★ What this does NOT fix, stated so it is not read as complete
    ///
    /// It closes the gap for the **default** intent, which is the shipped
    /// behaviour and the operator-visible case. A caller who has *changed*
    /// the intent still gets a decomposition that disagrees with their own
    /// renderer, because [`crate::vector::decompose`] takes no settings
    /// parameter at all — threading one through would change three public
    /// entry points and 57 call sites, which is its own Pass rather than a
    /// drive-by. [`Self::from_cmyk_with`] is the door for callers who have an
    /// intent in hand.
    #[must_use]
    pub fn from_cmyk(c: f32, m: f32, y: f32, k: f32) -> Self {
        Self::from_cmyk_with(crate::settings::CmykIntent::default(), c, m, y, k)
    }

    /// From DeviceCMYK components under an explicit
    /// [`CmykIntent`](crate::settings::CmykIntent).
    ///
    /// The mirror of `pdfcer_render::gstate::Rgb::from_cmyk`, so a caller
    /// holding a render policy can decompose to the same colours it will
    /// paint. See [`Self::from_cmyk`] for why the two ever differed.
    #[must_use]
    pub fn from_cmyk_with(
        intent: crate::settings::CmykIntent,
        c: f32,
        m: f32,
        y: f32,
        k: f32,
    ) -> Self {
        Self::from_triple(crate::color::cmyk_to_srgb_with(intent, c, m, y, k))
    }
}

// ---------------------------------------------------------------------------
// Shared path-construction primitives (the agree-by-construction anchor)
// ---------------------------------------------------------------------------
//
// These three pure functions encode the operand arithmetic of the three
// construction operators the PDF spec (§8.5.2.1, Table 59) defines by
// implication rather than literally — the exact places a second, forked
// decomposition would drift from the renderer. `pdfcer-render`'s
// interpreter calls them for the identical node values (narrowing the
// `f64` results to `f32`, which round-trips its `f32` operands exactly),
// so this object model and the render agree on these operators by sharing
// one implementation, not by two hand-derived copies staying in sync.

/// The three control/anchor points of the cubic the `v` operator appends
/// (`x2 y2 x3 y3 v`, Table 59): its **first control point is the current
/// point** — the classic "v/y trap" that silently mis-renders if forgotten.
///
/// Returns `(first_control, second_control, endpoint)` where
/// `first_control == current`.
#[must_use]
pub fn cubic_from_v(current: Point, x2: f64, y2: f64, x3: f64, y3: f64) -> (Point, Point, Point) {
    (current, Point::new(x2, y2), Point::new(x3, y3))
}

/// The three control/anchor points of the cubic the `y` operator appends
/// (`x1 y1 x3 y3 y`, Table 59): its **second control point is the
/// endpoint** — the mirror trap of `v`.
///
/// Returns `(first_control, second_control, endpoint)` where
/// `second_control == endpoint`.
#[must_use]
pub fn cubic_from_y(x1: f64, y1: f64, x3: f64, y3: f64) -> (Point, Point, Point) {
    let end = Point::new(x3, y3);
    (Point::new(x1, y1), end, end)
}

/// The four corner anchors of the rectangle the `re` operator appends
/// (`x y w h re`, Table 59), in the spec's defined expansion order
/// `(x,y) → (x+w,y) → (x+w,y+h) → (x,y+h)` closing back to `(x,y)`.
///
/// A negative `w`/`h` is legal and yields a rectangle traced the other
/// way — kept as-is (the caller's fill winding, not this function, is what
/// interprets orientation), exactly as `tiny-skia`'s `re` expansion does.
#[must_use]
pub fn rect_corners(x: f64, y: f64, w: f64, h: f64) -> [Point; 4] {
    [
        Point::new(x, y),
        Point::new(x + w, y),
        Point::new(x + w, y + h),
        Point::new(x, y + h),
    ]
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn approx(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    #[test]
    fn identity_maps_a_point_to_itself() {
        let p = Point::new(3.5, -7.25);
        assert!(approx(Matrix::IDENTITY.map_point(p), p));
    }

    #[test]
    fn map_point_uses_the_row_vector_formula() {
        // Scale 2 in x, 3 in y, translate (10, 20).
        let m = Matrix::new(2.0, 0.0, 0.0, 3.0, 10.0, 20.0);
        assert!(approx(
            m.map_point(Point::new(1.0, 1.0)),
            Point::new(12.0, 23.0)
        ));
    }

    #[test]
    fn post_concat_is_apply_self_then_other() {
        // A: translate by (5, 0). B: scale x by 2.
        let a = Matrix::translate(5.0, 0.0);
        let b = Matrix::new(2.0, 0.0, 0.0, 1.0, 0.0, 0.0);
        let composed = a.post_concat(b);
        let p = Point::new(1.0, 0.0);
        // apply A then B: (1,0) -> (6,0) -> (12,0)
        assert!(approx(composed.map_point(p), Point::new(12.0, 0.0)));
        // and it equals b.map(a.map(p)) by definition
        assert!(approx(composed.map_point(p), b.map_point(a.map_point(p))));
    }

    #[test]
    fn post_concat_matches_pdf_cm_premultiply_semantics() {
        // A 90° rotation composed with a translation, verifying the same
        // orientation the render interpreter's `m.post_concat(ctm)` uses:
        // rotating (1,0) by +90° gives (0,1); then translate (0,10).
        let rot = Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0);
        let tr = Matrix::translate(0.0, 10.0);
        let ctm = rot.post_concat(tr);
        assert!(approx(
            ctm.map_point(Point::new(1.0, 0.0)),
            Point::new(0.0, 11.0)
        ));
    }

    #[test]
    fn bounds_accumulate_and_ignore_non_finite() {
        let b = Bounds::EMPTY
            .union_point(Point::new(1.0, 2.0))
            .union_point(Point::new(-3.0, 5.0))
            .union_point(Point::new(f64::NAN, 0.0)); // ignored
        assert_eq!(b.min, Point::new(-3.0, 2.0));
        assert_eq!(b.max, Point::new(1.0, 5.0));
    }

    #[test]
    fn bounds_containment_and_intersection() {
        let outer = Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(10.0, 10.0),
        };
        let inner = Bounds {
            min: Point::new(2.0, 2.0),
            max: Point::new(4.0, 4.0),
        };
        let straddle = Bounds {
            min: Point::new(8.0, 8.0),
            max: Point::new(12.0, 12.0),
        };
        assert!(inner.contained_by(outer));
        assert!(!straddle.contained_by(outer));
        assert!(straddle.intersects(outer));
        assert!(outer.contains(Point::new(5.0, 5.0)));
        assert!(!outer.contains(Point::new(11.0, 5.0)));
    }

    #[test]
    fn v_operator_first_control_is_the_current_point() {
        let cur = Point::new(3.0, 4.0);
        let (c1, c2, end) = cubic_from_v(cur, 10.0, 11.0, 20.0, 21.0);
        assert_eq!(c1, cur);
        assert_eq!(c2, Point::new(10.0, 11.0));
        assert_eq!(end, Point::new(20.0, 21.0));
    }

    #[test]
    fn y_operator_second_control_is_the_endpoint() {
        let (c1, c2, end) = cubic_from_y(10.0, 11.0, 20.0, 21.0);
        assert_eq!(c1, Point::new(10.0, 11.0));
        assert_eq!(c2, Point::new(20.0, 21.0));
        assert_eq!(end, Point::new(20.0, 21.0));
        assert_eq!(c2, end);
    }

    #[test]
    fn inverse_undoes_map_point_for_a_rotate_scale_translate() {
        // A non-trivial affine: scale (2,3), 30° shear-ish, translate (7,-4).
        let m = Matrix::new(2.0, 0.5, -0.5, 3.0, 7.0, -4.0);
        let inv = m.inverse().expect("non-singular");
        for p in [
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(-3.5, 12.25),
        ] {
            let round = inv.map_point(m.map_point(p));
            assert!(approx(round, p), "inverse must undo map_point: {round:?}");
        }
    }

    #[test]
    fn map_vector_ignores_translation_and_matches_a_delta() {
        // A pure translation has identity linear part, so a delta is unchanged.
        let t = Matrix::translate(100.0, -50.0);
        assert!(approx(
            t.map_vector(Point::new(3.0, 4.0)),
            Point::new(3.0, 4.0)
        ));
        // Under a 2× scale a page-space delta of (10,10) is a user-space delta
        // of (5,5): inverse().map_vector recovers it.
        let m = Matrix::new(2.0, 0.0, 0.0, 2.0, 30.0, 30.0);
        let user_delta = m.inverse().unwrap().map_vector(Point::new(10.0, 10.0));
        assert!(approx(user_delta, Point::new(5.0, 5.0)));
    }

    #[test]
    fn a_singular_matrix_has_no_inverse() {
        // Rank-deficient (both rows collinear): determinant 0.
        assert!(
            Matrix::new(1.0, 2.0, 2.0, 4.0, 0.0, 0.0)
                .inverse()
                .is_none()
        );
        // Non-finite operands never yield an inverse.
        assert!(
            Matrix::new(f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0)
                .inverse()
                .is_none()
        );
    }

    #[test]
    fn re_corners_follow_the_spec_expansion_order() {
        let c = rect_corners(1.0, 2.0, 4.0, 3.0);
        assert_eq!(c[0], Point::new(1.0, 2.0));
        assert_eq!(c[1], Point::new(5.0, 2.0));
        assert_eq!(c[2], Point::new(5.0, 5.0));
        assert_eq!(c[3], Point::new(1.0, 5.0));
    }

    // -----------------------------------------------------------------
    // Pass 112.0 — the scale/rotate/about constructors
    //
    // These are the primitives every transform verb is built on, and each
    // one has a failure mode that produces a PLAUSIBLE WRONG PICTURE rather
    // than an error: a sign flip is a mirror image, a composition-order
    // slip is a shape that drifts a little on every drag. So the properties
    // are pinned, not the coefficients.
    // -----------------------------------------------------------------

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    /// Rotation is COUNTER-CLOCKWISE. A sign error on `c` is a mirror image,
    /// which renders perfectly and is wrong, so the direction is pinned at
    /// all four quarter turns rather than one.
    #[test]
    fn rotation_is_counter_clockwise() {
        use std::f64::consts::{FRAC_PI_2, PI};
        let x = Point::new(1.0, 0.0);
        assert!(close(
            Matrix::rotate(FRAC_PI_2).map_point(x),
            Point::new(0.0, 1.0)
        ));
        assert!(close(
            Matrix::rotate(PI).map_point(x),
            Point::new(-1.0, 0.0)
        ));
        assert!(close(
            Matrix::rotate(3.0 * FRAC_PI_2).map_point(x),
            Point::new(0.0, -1.0)
        ));
        assert!(close(Matrix::rotate(2.0 * PI).map_point(x), x));
    }

    /// ★ The defining property of `about`: the pivot does not move. It holds
    /// for rotation, uniform scale, non-uniform scale and a composition of
    /// them, and it is the one assertion that catches a reversed
    /// `post_concat` order — which otherwise looks like a small drift.
    #[test]
    fn about_fixes_its_pivot_for_every_transform() {
        let pivot = Point::new(37.5, -12.25);
        for m in [
            Matrix::rotate(0.7),
            Matrix::scale(3.0, 3.0),
            Matrix::scale(2.0, 0.5),
            Matrix::rotate(-1.3).post_concat(Matrix::scale(1.5, 4.0)),
        ] {
            let moved = m.about(pivot).map_point(pivot);
            assert!(close(moved, pivot), "pivot moved to {moved:?} under {m:?}");
        }
    }

    /// `about` must agree with the long-hand it is shorthand for. Written as
    /// an independent reference implementation rather than a restatement:
    /// asserting `about` against its own formula would prove nothing.
    #[test]
    fn about_agrees_with_translate_transform_translate_back() {
        let pivot = Point::new(-4.0, 9.0);
        let m = Matrix::rotate(0.4).post_concat(Matrix::scale(2.0, 3.0));
        for p in [
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(-7.5, 22.0),
            pivot,
        ] {
            let via_about = m.about(pivot).map_point(p);
            // The long-hand, done in three separate steps on the POINT.
            let shifted = Point::new(p.x - pivot.x, p.y - pivot.y);
            let turned = m.map_point(shifted);
            let back = Point::new(turned.x + pivot.x, turned.y + pivot.y);
            assert!(close(via_about, back), "{p:?}: {via_about:?} vs {back:?}");
        }
    }

    /// A scale about a pivot moves every OTHER point by the factor, measured
    /// from the pivot — the property a resize grip actually relies on.
    #[test]
    fn scaling_about_a_pivot_scales_distance_from_it() {
        let pivot = Point::new(100.0, 100.0);
        let m = Matrix::scale(3.0, 3.0).about(pivot);
        let p = m.map_point(Point::new(110.0, 100.0));
        assert!(close(p, Point::new(130.0, 100.0)), "got {p:?}");
    }

    /// Non-uniform scale must not secretly rotate: a horizontal edge stays
    /// horizontal.
    #[test]
    fn non_uniform_scale_does_not_rotate() {
        let m = Matrix::scale(4.0, 0.25);
        let a = m.map_point(Point::new(0.0, 5.0));
        let b = m.map_point(Point::new(10.0, 5.0));
        assert!((a.y - b.y).abs() < 1e-9, "the edge tilted: {a:?} {b:?}");
        assert!((b.x - a.x - 40.0).abs() < 1e-9);
    }

    /// ★ `is_invertible` answers the question a shell needs BEFORE it offers
    /// a resize grip, and separately from "this object has no placement".
    /// The drag-through-zero case is the one the consuming shell named.
    #[test]
    fn is_invertible_refuses_exactly_the_degenerate_matrices() {
        assert!(Matrix::IDENTITY.is_invertible());
        assert!(Matrix::rotate(1.0).is_invertible());
        assert!(Matrix::scale(2.0, 0.5).is_invertible());
        assert!(Matrix::translate(10.0, -3.0).is_invertible());

        // Dragged through zero — the case that must be a NAMED refusal.
        assert!(!Matrix::scale(0.0, 1.0).is_invertible());
        assert!(!Matrix::scale(1.0, 0.0).is_invertible());
        assert!(!Matrix::scale(0.0, 0.0).is_invertible());
        // Collapsed onto a line by a shear.
        assert!(!Matrix::new(1.0, 2.0, 2.0, 4.0, 0.0, 0.0).is_invertible());
        // Non-finite must never reach a `cm` operand.
        assert!(!Matrix::scale(f64::NAN, 1.0).is_invertible());
        assert!(!Matrix::scale(f64::INFINITY, 1.0).is_invertible());
        assert!(!Matrix::new(1.0, 0.0, 0.0, 1.0, f64::NAN, 0.0).is_invertible());
    }

    /// `is_invertible` must agree with `inverse` — two answers to one
    /// question that could drift apart otherwise.
    #[test]
    fn is_invertible_agrees_with_inverse() {
        for m in [
            Matrix::IDENTITY,
            Matrix::rotate(0.3),
            Matrix::scale(2.0, 3.0),
            Matrix::scale(0.0, 1.0),
            Matrix::new(1.0, 2.0, 2.0, 4.0, 0.0, 0.0),
            Matrix::scale(f64::NAN, 1.0),
        ] {
            assert_eq!(
                m.is_invertible(),
                m.inverse().is_some(),
                "the predicate and the operation disagree about {m:?}"
            );
        }
    }
}

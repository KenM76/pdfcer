//! # Dimension groups + the authoritative model (decision 011 §2.4)
//!
//! The named-group model that carries per-group **scale + units + number
//! format + OCG layer + membership** — pdfcer's *authoritative* dimensioning
//! state (the `/PieceInfo` sidecar; the portable `/Measure` dict is a
//! projection of it, decision 011 §2.4). It is pure data over the value model
//! ([`super::units`]) and the fit ([`super::fit`]); the storage layer
//! ([`super::sidecar`], [`super::measure_dict`]) serialises it and
//! [`crate::edit`] wires the resulting objects into a document.
//!
//! ## Non-geometric grouping — a genuine exceed-Acrobat (ui-spec §0.2)
//!
//! A [`Group`] is a *named* entity any dimension can join **regardless of its
//! page position**. Acrobat's own `/Viewport` scoping is a hard geometric
//! partition of the page; pdfcer's authoritative group model is not, and this
//! is why the portable `/Measure` mirror can only ever be a best-effort
//! projection for overlapping different-scale groups on one page (decision
//! 011 §2.4 "native_scale") — the sidecar is the authority.
//!
//! ## A default group always exists (ui-spec §5.3)
//!
//! [`DimensionModel::new`] seeds exactly one group, [`DEFAULT_GROUP_ID`],
//! named "Default", scale never-set — so a dimension always has a home and the
//! group panel is never empty. The default group is un-hideable (hiding the
//! only fallback group is a foot-gun with no benefit — ui-spec §5.3).

use crate::object::ObjId;
use crate::vector::{AxisConstraint, Point, measured_length, polyline_length};

use super::fit::FitCircle;
use super::units::{
    MeasurementDisplay, NumberFormat, ScaleState, Unit, format_angle_degrees, format_measurement,
};

/// A stable dimension-group identifier (index-independent, so the sidecar can
/// reference a group across a save/reload without depending on Vec order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(pub u32);

/// A stable per-dimension identifier within the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DimensionId(pub u32);

/// The always-present default/active group (ui-spec §5.3).
pub const DEFAULT_GROUP_ID: GroupId = GroupId(0);

/// A named dimension group: scale + units + number format + its optional-
/// content layer + default visibility (decision 011 §2.4).
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// Stable identifier.
    pub id: GroupId,
    /// Operator-facing name (e.g. "Floor Plan").
    pub name: String,
    /// The tri-state scale (never-set / explicit-1:1 / calibrated) — ui-spec
    /// §4.3, so a legit 1:1 is distinct from uncalibrated.
    pub scale: ScaleState,
    /// The number format (carries the group's display [`Unit`]).
    pub format: NumberFormat,
    /// The group's Optional Content Group object, once authored into a
    /// document (`None` before the first dimension is saved).
    pub ocg: Option<ObjId>,
    /// The layer's **default visibility** (the `/D` config state): `true` ⇒
    /// the OCG is ON by default; `false` ⇒ registered in `/D /OFF` (hidden).
    pub visible: bool,
    /// The drafting standard this group's ce dimensions are DRAWN to
    /// (Pass 27.2): terminator form, whether the dimension line is broken for
    /// the text, text orientation, and whether the extension-line gap and
    /// overshoot are absolute or line-width-relative.
    ///
    /// # Why per group
    ///
    /// The group already owns every other display-governing property — scale,
    /// unit, number format, layer — and the standard is the same class of
    /// thing. Per ce dimension would be a foot-gun with no use case (nobody
    /// wants dimension #3 ISO and #4 ANSI); per document would mean inventing
    /// a document tier, and merge semantics for it, for one field. The unit
    /// interaction settles it: the standards' decimal conventions are
    /// unit-dependent, and the unit is per group.
    ///
    /// Changing it regenerates every member, exactly like a scale change — a
    /// group exists so its members agree.
    pub standard: DimStandard,
    /// **The group's appearance defaults** (`Pass 69.0`) — the middle tier of
    /// the style cascade ([`super::style`]).
    ///
    /// Every field is an `Option`: `None` means the group has not spoken and
    /// the factory default applies. A member that overrides nothing follows
    /// this; a member that overrides ONE property still follows this for the
    /// rest, which is the whole point of per-property inheritance.
    ///
    /// The unit, number format, decimal marker, drafting standard and scale
    /// are NOT duplicated here — they are the concrete fields above, and this
    /// group's tier for them. Two homes for one property is how they drift.
    pub style: super::style::GroupStyle,
}

/// The drafting standard governing how ce dimensions are DRAWN (Pass 27.2).
///
/// Does not govern the numeric string, with one researched exception: ISO
/// 129-1:2018 cl. 4.1.1 mandates a comma decimal marker, which lives on
/// [`NumberFormat`] and is SET from this as a disclosed side effect rather
/// than implied by it.
///
/// **pdfcer draws "ISO-style", never "ISO 129-1 conformant".** ISO 129-1's
/// normative Annex A (symbol proportions against lettering height, ISO 3098
/// class B) is paywalled and was not obtained, so conformance cannot honestly
/// be claimed — only the practice that was verifiable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DimStandard {
    /// ANSI/ASME practice — the factory default (operator, 2026-08-04: "My
    /// default is ANSI, but ISO should be an option too").
    ///
    /// Line/arrowhead and lettering conventions are **ASME Y14.2**, not
    /// Y14.5: Y14.5 is the GD&T/tolerancing standard and is routinely
    /// miscited for this. No clause number is given here because the document
    /// is paywalled and was not obtained.
    #[default]
    Ansi,
    /// ISO 129-1 practice.
    Iso,
}

impl Group {
    /// A fresh group with `unit`'s default number format, never-set scale, and
    /// default (visible) layer state.
    #[must_use]
    pub fn new(id: GroupId, name: impl Into<String>, unit: Unit) -> Self {
        Self {
            id,
            name: name.into(),
            scale: ScaleState::NeverSet,
            format: unit.default_format(),
            ocg: None,
            visible: true,
            standard: DimStandard::default(),
            style: super::style::GroupStyle::default(),
        }
    }

    /// The group's display unit (from its [`NumberFormat`]).
    #[must_use]
    pub const fn unit(&self) -> Unit {
        self.format.unit
    }
}

/// The immutable geometry a dimension stores (decision 011 §2.3 value model:
/// geometry is stored, the displayed value is derived). Radius vs diameter is
/// a *display-only* toggle on the same [`Self::Circular`] geometry (ui-spec
/// §1.1) — never a separate fit.
///
/// # ★ NOT `Copy`, since `Pass 107.0`, and that is deliberate
///
/// Every variant up to `Pass 106.x` was fixed-arity, so the enum was `Copy`
/// and roughly a hundred call sites across four crates dereferenced it with
/// `*kind` for free. [`Self::Perimeter`] holds a `Vec<Point>` and cannot be,
/// so the derive was dropped rather than the variant reshaped.
///
/// The alternative was offered by the consuming shell and considered
/// seriously: keep the enum `Copy` by holding the vertices in
/// [`DimensionModel`] and referencing them from the variant by id. It was
/// **refused**, because it splits ONE ce dimension's geometry across two
/// containers. The sidecar would serialise two arrays that must stay in step,
/// [`crate::edit::EditSession`]'s undo log would have to revert two of them
/// atomically, and the invariant this whole subsystem exists to hold — *the
/// drawn shape is exactly the thing measured* — would become a join between
/// two tables rather than a property of one value. A bounded inline array was
/// refused for the shell's own stated reason: a footprint traced around a
/// building runs to thirty-odd vertices and any cap is a cap somebody hits.
///
/// `Copy` is a promise that a bit-copy is the right way to duplicate a value.
/// A thirty-vertex polyline is not that, so the honest type is not `Copy` and
/// the ~40 mechanical `.clone()`s are the price of saying so once.
#[derive(Debug, Clone, PartialEq)]
pub enum DimensionKind {
    /// A linear dimension between two page-space points under an axis
    /// constraint. The stored measured length is
    /// `measured_length(a, b, constraint)` (points).
    Linear {
        /// First picked point (page space, points).
        a: Point,
        /// Second picked point (page space, points).
        b: Point,
        /// The alignment constraint (Aligned / Horizontal / Vertical).
        constraint: AxisConstraint,
        /// Signed standoff of the dimension line from point `a`, along the
        /// constraint's canonical normal, in points (Pass 27.0).
        ///
        /// # Why a scalar based at `a`, and why the default is exactly zero
        ///
        /// The dimension line does not run between the picked points — it runs
        /// PARALLEL to the constraint axis, offset away from the drawing, with
        /// extension (witness) lines reaching back to the points. This is the
        /// distance it stands off by. Positive is up (horizontal dimensions)
        /// or right (vertical ones); see [`DimensionKind::axis_frame`], whose
        /// normal is canonicalised so the sign does not depend on which point
        /// the operator clicked first.
        ///
        /// The default is **0.0**, which places the dimension line through `a`
        /// — reproducing exactly what the tool's own preview already draws. So
        /// an existing ce dimension deserialised without this key looks
        /// identical to how it looked before the field existed, and the
        /// sidecar migration costs nothing.
        offset: f64,
        /// Signed position of the value TEXT along the dimension line,
        /// measured from its midpoint, in points (Pass 27.1).
        ///
        /// # Why this is a second field and not folded into `offset`
        ///
        /// SolidWorks stores a dimension's placement as a POINT — its API
        /// takes one (`AddDimension2(x, y, z)`, "the text-placement point").
        /// In the dimension's own frame that point has two components: how far
        /// the line stands off the drawing (perpendicular, [`Self::Linear`]'s
        /// `offset`) and where the number sits along it (parallel, this). One
        /// drag sets both, which is what makes a SolidWorks dimension drag feel
        /// like moving an object rather than operating two controls.
        ///
        /// Keeping them as separate scalars rather than storing the raw point
        /// means neither can drift out of the frame when the picks or the
        /// constraint change — the point would have to be re-projected, and a
        /// re-projection that ever disagreed with the geometry would put the
        /// text somewhere the operator never dropped it.
        ///
        /// 0.0 is centred, which is where every dimension authored before this
        /// field existed puts its label.
        text_along: f64,
    },
    /// A radius/diameter dimension over a best-fit circle. Stores the fit
    /// (centre + radius + residual — the residual surfaced per decision 011
    /// §2.3) and whether the DISPLAY is diameter (`2×radius`) or radius.
    Circular {
        /// The fitted circle (page space, points).
        fit: FitCircle,
        /// `true` ⇒ display the diameter; `false` ⇒ the radius.
        show_diameter: bool,
    },
    /// An ANGLE between two lines the operator picked (`Pass 68.0`).
    ///
    /// # Why the rays are stored, not the two source lines
    ///
    /// The two picked lines bound four angles, and which one the operator
    /// meant was decided at pick time by where they clicked
    /// ([`crate::vector::linepick`]). Storing the lines would throw that
    /// decision away and force every later regeneration to re-derive it —
    /// re-deriving a choice the operator already made is how a dimension
    /// silently becomes a different dimension after a scale change.
    ///
    /// So the resolved answer is stored: the apex, and a unit direction along
    /// each arm pointing INTO the measured wedge. The angle is the one
    /// between those two rays, always, with no further interpretation.
    ///
    /// # ★ An angle does NOT scale
    ///
    /// This is the one place the ce-dimension value model genuinely differs.
    /// [`Self::measured_points`] returns a length that the group's scale
    /// multiplies — 100 points at 1:50 is 5000 units. An angle is invariant
    /// under uniform scaling: 30° on a drawing at 1:50 is 30°, not 1500 of
    /// anything. [`DimensionModel::display`] therefore branches here and never
    /// applies scale, and [`Self::measured_points`] returns the DEGREES so no
    /// caller can silently feed an angle into the length formatter and get a
    /// plausible, wrong number out.
    Angular {
        /// Where the two lines cross, page space. May be virtual — outside
        /// both picked segments — which is normal in CAD drawings.
        apex: Point,
        /// Unit direction of the first arm, from `apex` into the wedge.
        dir_a: Point,
        /// Unit direction of the second arm, from `apex` into the wedge.
        dir_b: Point,
        /// Radius of the dimension ARC from the apex, in points.
        ///
        /// The angular analogue of [`Self::Linear`]'s `offset`: how far the
        /// drawn arc stands off from the vertex. Same role, same one-drag
        /// placement model, different geometry — an arc has a radius where a
        /// linear dimension has a perpendicular standoff.
        radius: f64,
        /// Signed position of the value text ALONG the arc, in degrees from
        /// the arc's midpoint.
        ///
        /// Degrees rather than points so the label keeps its position on the
        /// arc when the radius changes, which is what an operator expects
        /// when they drag the arc further out.
        text_along: f64,
    },
    /// A **perimeter / path length** over a picked polyline (`Pass 107.0`) —
    /// the sum of every segment, printed as ONE number.
    ///
    /// # The operator's ask, and why it could not be composed
    ///
    /// > *"give me perimeter measuring tool as well where I click around to
    /// > make a shape and it adds the distance of all the segments together
    /// > for the dimension display … this should come with all the scaling
    /// > options of the other dimension tools."*
    /// > — Ken, 2026-08-20
    ///
    /// The last clause is the one that decides this is a [`DimensionKind`]
    /// rather than a markup annotation carrying a number in its `/Contents`:
    /// group scale, unit, [`NumberFormat`], drafting standard, layer and the
    /// `Pass 69.0` style cascade all hang off a ce dimension's group, and a
    /// number the engine does not own is a caption rather than a measurement
    /// — it cannot be re-measured when the group's scale is calibrated later.
    ///
    /// Three chained [`Self::Linear`] ce dimensions would print three labels
    /// where the ask is one, and a shared endpoint would move in two commands
    /// so an undo could leave the chain broken. A single [`Self::Linear`]
    /// whose `a`/`b` were stretched to fake the total would fabricate
    /// geometry — the drawn line would not be the thing measured, which is
    /// the exact invariant [`Self::linear_geometry`]'s doc comment exists to
    /// hold.
    ///
    /// # Open and closed are ONE kind with a flag, not two kinds
    ///
    /// `closed: true` is a perimeter (a fence run around a footprint);
    /// `closed: false` is a path length (a pipe run, a cable route). They
    /// differ by exactly one segment — the one from the last vertex back to
    /// the first — and by the annotation subtype that carries them
    /// (`/Polygon` vs `/PolyLine`, ISO 32000-1 §12.5.6.9 Table 178). Every
    /// other property, every verb and every formatting path is identical, so
    /// splitting them into two variants would duplicate all of that to
    /// express one boolean.
    Perimeter {
        /// The picked vertices, in pick order, page space, points.
        ///
        /// **At least two** for an open path and **at least three** for a
        /// closed one — enforced at the verbs that can shorten the list
        /// ([`crate::edit::EditSession::remove_dimension_vertex`]) rather
        /// than by this type, because a `Vec` cannot express the bound and a
        /// wrapper that could would have to be constructible by the shell
        /// anyway. [`Self::measured_points`] is defined for a shorter list
        /// (it sums what is there, which is 0.0 for one vertex); it is the
        /// DRAWING and the file that would be degenerate.
        points: Vec<Point>,
        /// Whether the last vertex joins back to the first.
        ///
        /// When `true` the closing segment is part of the measured total and
        /// the annotation is authored as a `/Polygon`; when `false` it is
        /// not, and the annotation is a `/PolyLine`.
        closed: bool,
        /// The label's displacement from the vertex centroid, ALONG PAGE +y.
        ///
        /// # Why the centroid, and why page axes rather than a shape axis
        ///
        /// [`Self::Linear`] measures the standoff along its own constraint
        /// normal, because a linear dimension HAS one axis and the number
        /// belongs beside it. A polyline has as many axes as it has segments.
        /// The two candidate conventions were *the longest segment's frame*
        /// (CAD-conventional) and *the vertex centroid* — and the deciding
        /// property is that this kind's headline feature is **vertex
        /// editing**. Under the longest-segment convention, dragging one
        /// corner can change WHICH segment is longest, and the label
        /// teleports across the shape for a reason no operator can see.
        /// Under the centroid convention the label drifts smoothly with the
        /// shape, which is what dragging a corner should look like.
        ///
        /// So the anchor is `centroid + (text_along, offset)` in page axes,
        /// which also means [`crate::edit::EditSession::place_dimension`]
        /// carries this kind with no new fields and no new semantics: a drag
        /// still resolves to one `(offset, text_along)` pair.
        offset: f64,
        /// The label's displacement from the vertex centroid, ALONG PAGE +x.
        ///
        /// Named `text_along` to match [`Self::Linear`] and [`Self::Angular`]
        /// so [`crate::edit::EditSession::place_dimension`]'s two arguments
        /// mean the same thing on every placeable kind. See [`Self::Perimeter::offset`]
        /// for why the pair is page-axis-relative here and frame-relative
        /// there.
        text_along: f64,
    },
}

impl DimensionKind {
    /// This dimension's displayed value under a group's scale and format.
    ///
    /// # ★ The one place a ce dimension's value becomes text
    ///
    /// [`DimensionModel::display`] and [`crate::dimension::author_dimension`]
    /// both need this. Until `Pass 68.0` they each computed it, and only the
    /// first one had the angular branch — so the pane read `77.5°` while the
    /// label baked into the page's `/AP` read **`77.47 pt`**, an angle run
    /// through the length formatter and given a unit it does not have. That
    /// is the exact failure [`Self::Angular`]'s own docs warn about, arrived
    /// at through the writer rather than the reader.
    ///
    /// One producer now. A second copy of a branch this easy to omit is how
    /// the number an operator reads and the number the file carries come
    /// apart, and the file's copy is the one that outlives the session.
    ///
    /// # An angle bypasses the scale entirely
    ///
    /// 30° on a drawing at 1:50 is 30°. Multiplying by the scale would produce
    /// 1500 of nothing, and `raw_page_units` would be `false`, so the wrong
    /// number would arrive carrying no disclosure that anything was odd.
    #[must_use]
    pub fn display_with(&self, scale: ScaleState, format: NumberFormat) -> MeasurementDisplay {
        if self.is_angular() {
            return MeasurementDisplay {
                text: format_angle_degrees(self.measured_points(), format),
                raw_page_units: false,
            };
        }
        format_measurement(self.measured_points(), scale, format)
    }

    /// The dimension line's own frame: the unit vector along the axis being
    /// measured, and the canonical normal the standoff is measured along
    /// (Pass 27.0).
    ///
    /// # Why this exists as its own function
    ///
    /// The constraint decides the direction the dimension line RUNS, and until
    /// Pass 27.0 nothing consumed that fact: `leader_endpoints` returned the
    /// two picked points verbatim, so a dimension constrained to Horizontal
    /// was drawn at whatever angle the operator's two clicks happened to make.
    /// The value was right and the line was wrong — the operator's report,
    /// 2026-08-04: *"It looks like it give me the correct horizontal or
    /// vertical dimension but it shows at an angle."*
    ///
    /// # The canonical normal, and why the sign must not depend on click order
    ///
    /// The normal is the axis rotated a quarter turn counter-clockwise, then
    /// flipped if necessary so it points up (or right, for a vertical
    /// dimension). Without that flip, clicking right-to-left instead of
    /// left-to-right would negate the normal, and the same positive standoff
    /// would put the dimension line on the opposite side of the drawing. An
    /// operator would experience that as the offset control working backwards
    /// half the time, for no reason they could see.
    ///
    /// Returns `None` for a degenerate `Aligned` dimension (two coincident
    /// picks), which has no axis to speak of — refused rather than fabricated.
    #[must_use]
    pub fn axis_frame(&self) -> Option<(Point, Point)> {
        let Self::Linear {
            a, b, constraint, ..
        } = *self
        else {
            return None;
        };
        let u = match constraint {
            AxisConstraint::Horizontal => Point::new(1.0, 0.0),
            AxisConstraint::Vertical => Point::new(0.0, 1.0),
            AxisConstraint::Aligned => {
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let len = dx.hypot(dy);
                if !len.is_finite() || len <= f64::EPSILON {
                    return None;
                }
                Point::new(dx / len, dy / len)
            }
        };
        // Perpendicular, counter-clockwise, then canonicalised into the upper
        // half-plane (ties broken toward +x) so the sign of `offset` means the
        // same thing regardless of pick order.
        let mut n = Point::new(-u.y, u.x);
        if n.y < 0.0 || (n.y == 0.0 && n.x < 0.0) {
            n = Point::new(-n.x, -n.y);
        }
        Some((u, n))
    }

    /// Where the dimension LINE runs, and where each extension line reaches
    /// from — everything the appearance needs (Pass 27.0).
    ///
    /// Returns `(dim_a, dim_b, ext_from_a, ext_from_b)`: the two ends of the
    /// dimension line, and the two measured points the extension lines run to.
    ///
    /// # The invariant this exists to hold
    ///
    /// `|dim_b - dim_a|` equals [`Self::measured_points`], always. The drawn
    /// line is exactly as long as the number printed on it. Before Pass 27.0 a
    /// Horizontal dimension whose picks differed in y drew `hypot(dx, dy)`
    /// while its label said `dx` — a line that disagreed with its own caption,
    /// which is the specific thing an operator cannot be expected to catch.
    ///
    /// Where the value TEXT is anchored: the midpoint of the dimension line,
    /// slid along it by `text_along` (Pass 27.1).
    ///
    /// `None` for a non-linear or degenerate dimension.
    #[must_use]
    pub fn label_anchor(&self) -> Option<Point> {
        // A perimeter anchors on its vertex centroid, displaced by the same
        // `(text_along, offset)` pair every placeable kind carries — but in
        // PAGE axes, because a polyline has no single frame to be relative to.
        // See `Perimeter::offset` for why the centroid rather than the longest
        // segment: vertex editing is this kind's headline feature, and the
        // longest segment can change identity mid-drag.
        if let Self::Perimeter {
            offset, text_along, ..
        } = *self
        {
            let c = self.polyline_centroid()?;
            return Some(Point::new(c.x + text_along, c.y + offset));
        }
        let Self::Linear { text_along, .. } = *self else {
            return None;
        };
        let (u, _) = self.axis_frame()?;
        let (dim_a, dim_b, _, _) = self.linear_geometry()?;
        let mid = dim_a.midpoint(dim_b);
        Some(Point::new(
            mid.x + text_along * u.x,
            mid.y + text_along * u.y,
        ))
    }

    /// Set the placement from a pointer position in page space — the
    /// SolidWorks drag, resolved into this dimension's own frame (Pass 27.1).
    ///
    /// Returns the `(offset, text_along)` pair the point implies. The picked
    /// points are NOT touched: in SolidWorks, dragging a dimension never
    /// re-measures it, it only decides where the dimension is drawn. That
    /// separation is what makes the gesture safe enough to be the default —
    /// it writes fields the value function does not read, so the number cannot
    /// change no matter where the operator drops it.
    ///
    /// `None` for a non-linear or degenerate dimension.
    #[must_use]
    pub fn placement_from_point(&self, p: Point) -> Option<(f64, f64)> {
        // A perimeter's placement pair IS the page-space displacement from the
        // centroid, so the drag resolves with no projection at all — and,
        // unlike the linear case, dropping the label anywhere is expressible
        // rather than being flattened onto one axis.
        if let Self::Perimeter { .. } = *self {
            let c = self.polyline_centroid()?;
            return Some((p.y - c.y, p.x - c.x));
        }
        let Self::Linear { a, b, .. } = *self else {
            return None;
        };
        let (u, n) = self.axis_frame()?;
        // Perpendicular component, measured from `a` — the standoff.
        let offset = (p.x - a.x) * n.x + (p.y - a.y) * n.y;
        // Parallel component, measured from the MIDPOINT of the measured
        // extent, because that is where a centred label sits.
        let t = (b.x - a.x) * u.x + (b.y - a.y) * u.y;
        let along = ((p.x - a.x) * u.x + (p.y - a.y) * u.y) - t / 2.0;
        Some((offset, along))
    }

    /// `None` for a non-linear or degenerate dimension.
    #[must_use]
    pub fn linear_geometry(&self) -> Option<(Point, Point, Point, Point)> {
        let Self::Linear { a, b, offset, .. } = *self else {
            return None;
        };
        let (u, n) = self.axis_frame()?;
        // Projection of the pick-to-pick vector onto the axis: the measured
        // extent. Its SIGN matters — it decides which way the terminators
        // point — so it is not taken as an absolute here.
        let t = (b.x - a.x) * u.x + (b.y - a.y) * u.y;
        let o = Point::new(offset * n.x, offset * n.y);
        let dim_a = Point::new(a.x + o.x, a.y + o.y);
        let dim_b = Point::new(dim_a.x + t * u.x, dim_a.y + t * u.y);
        Some((dim_a, dim_b, a, b))
    }

    /// Rotate every stored point about `pivot` by `radians` anticlockwise
    /// (`Pass 159.0`) — the isometry sibling of [`Self::translated`].
    ///
    /// # ★★★ The measured value does NOT change, and that is the whole point
    ///
    /// A rotation preserves every distance and every angle. So the number a ce
    /// dimension displays is **identical before and after**, by construction
    /// rather than by pdfcer choosing to keep it — and that is what makes
    /// rotating one a legitimate drafting operation while *scaling* one is not.
    ///
    /// ★ **Scaling a ce dimension is deliberately not offered.** It has no
    /// honest reading. Either the value stays fixed while the geometry grows,
    /// so the dimension lies about the drawing; or both change, so nothing was
    /// measured and the operator has drawn a number rather than taken one. The
    /// operation an operator actually wants is
    /// [`EditSession::set_group_scale`](crate::edit::EditSession::set_group_scale),
    /// which changes the measurement RATIO — points per unit — and already
    /// ships. Rule 15's distinction is exactly this: a ce dimension's text IS
    /// its measurement.
    ///
    /// # ★★ `dir_a` / `dir_b` are the one place this differs from a translate
    ///
    /// [`Self::translated`] leaves an `Angular`'s arm directions alone,
    /// because they are **unit vectors rather than points** and translating
    /// them would rotate the dimension — its own comment says so.
    ///
    /// A rotation is the mirror case: the arms **must** turn, and they must
    /// turn WITHOUT the pivot's translation component, or a dimension away
    /// from the origin would have its arms flung across the page while its
    /// apex moved correctly. So the apex is mapped through the full
    /// pivot-rotation and the directions through the bare rotation.
    ///
    /// # What is left alone, and why it is correct rather than lazy
    ///
    /// `offset`, `text_along` and `radius` are all measured **relative to the
    /// dimension's own geometry** — a standoff along the normal, a fraction
    /// along the line, a distance from the apex. Rotating the geometry carries
    /// its own frame with it, so every one of them still means what it meant.
    /// This is the same reasoning `translated` gives for leaving them.
    ///
    /// `constraint` is **not** handled here; it is a caller-level decision
    /// because it can be invalidated by the rotation. See
    /// [`EditSession::rotate_dimension`](crate::edit::EditSession::rotate_dimension).
    #[must_use]
    pub fn rotated(self, pivot: Point, radians: f64) -> Self {
        let (sin, cos) = radians.sin_cos();
        // About the pivot: translate to origin, rotate, translate back.
        let map = |p: Point| {
            let (x, y) = (p.x - pivot.x, p.y - pivot.y);
            Point::new(pivot.x + x * cos - y * sin, pivot.y + x * sin + y * cos)
        };
        // Bare rotation, for direction vectors -- no translation component.
        let spin = |p: Point| Point::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos);

        match self {
            Self::Linear {
                a,
                b,
                constraint,
                offset,
                text_along,
            } => Self::Linear {
                a: map(a),
                b: map(b),
                constraint,
                offset,
                text_along,
            },
            Self::Circular { fit, show_diameter } => Self::Circular {
                fit: FitCircle {
                    center: map(fit.center),
                    ..fit
                },
                show_diameter,
            },
            Self::Angular {
                apex,
                dir_a,
                dir_b,
                radius,
                text_along,
            } => Self::Angular {
                apex: map(apex),
                // The arms turn, and they turn about the ORIGIN, not the
                // pivot -- see this method's docs.
                dir_a: spin(dir_a),
                dir_b: spin(dir_b),
                radius,
                text_along,
            },
            Self::Perimeter {
                points,
                closed,
                offset,
                text_along,
            } => Self::Perimeter {
                points: points.into_iter().map(map).collect(),
                closed,
                offset,
                text_along,
            },
        }
    }

    /// This geometry translated by a page-space `(dx, dy)`.
    ///
    /// # Why the measured value is untouched, on purpose
    ///
    /// A translation is a rigid motion: every distance it preserves. So a
    /// moved ce dimension reads exactly the same as before, and that is
    /// correct rather than a shortcut — moving a dimension repositions the
    /// annotation on the page, it does not re-measure anything. An operator
    /// who wants a different value is picking different points, which is a
    /// different operation.
    ///
    /// A `Circular` dimension moves its fitted CENTRE and keeps its radius,
    /// for the same reason; the residual is a property of the fit that
    /// produced it and is carried unchanged.
    #[must_use]
    pub fn translated(self, dx: f64, dy: f64) -> Self {
        match self {
            Self::Linear {
                a,
                b,
                constraint,
                offset,
                text_along,
            } => Self::Linear {
                a: Point::new(a.x + dx, a.y + dy),
                b: Point::new(b.x + dx, b.y + dy),
                constraint,
                // Both placement components are relative to the picked points,
                // which moved with the dimension, so a translation leaves them
                // untouched.
                offset,
                text_along,
            },
            Self::Circular { fit, show_diameter } => Self::Circular {
                fit: FitCircle {
                    center: Point::new(fit.center.x + dx, fit.center.y + dy),
                    ..fit
                },
                show_diameter,
            },
            // Only the apex moves. The arm DIRECTIONS are unit vectors, not
            // points — translating them would rotate the dimension, which is
            // not what a move is. Radius and text position are relative to
            // the apex and travel with it.
            Self::Angular {
                apex,
                dir_a,
                dir_b,
                radius,
                text_along,
            } => Self::Angular {
                apex: Point::new(apex.x + dx, apex.y + dy),
                dir_a,
                dir_b,
                radius,
                text_along,
            },
            // Every vertex moves; the placement pair does not. The label is
            // held relative to the CENTROID, and a translation moves the
            // centroid by exactly the same vector, so a perimeter's label
            // travels with its shape for free — the same reason
            // `Linear`'s pair is untouched above.
            Self::Perimeter {
                points,
                closed,
                offset,
                text_along,
            } => Self::Perimeter {
                points: points
                    .into_iter()
                    .map(|p| Point::new(p.x + dx, p.y + dy))
                    .collect(),
                closed,
                offset,
                text_along,
            },
        }
    }

    /// The stored geometry length in PDF points that the display value scales
    /// (`measured_points × scale`). Linear = the constrained length; Circular
    /// = radius, or `2×radius` when displaying diameter.
    #[must_use]
    pub fn measured_points(&self) -> f64 {
        match *self {
            DimensionKind::Linear {
                a, b, constraint, ..
            } => measured_length(a, b, constraint),
            DimensionKind::Circular { fit, show_diameter } => {
                if show_diameter {
                    2.0 * fit.radius
                } else {
                    fit.radius
                }
            }
            // ★ DEGREES, not points, and deliberately so. An angle is
            // invariant under uniform scaling, so there is no "length in page
            // points" that a group scale could legitimately multiply. Every
            // caller that formats a length must therefore ask
            // `is_angular()` first — see `DimensionModel::display`, which
            // does. Returning a plausible length here instead would let an
            // angle be silently scaled into a wrong number that still looked
            // like a measurement.
            DimensionKind::Angular { dir_a, dir_b, .. } => {
                let dot = dir_a.x.mul_add(dir_b.x, dir_a.y * dir_b.y).clamp(-1.0, 1.0);
                dot.acos().to_degrees()
            }
            // The whole point of the kind: ONE number, summed over every
            // segment, in page points, so the group's scale multiplies it
            // exactly as it multiplies a linear length. A perimeter IS a
            // length, which is why no new number formatting was needed.
            DimensionKind::Perimeter {
                ref points, closed, ..
            } => polyline_length(points, closed),
        }
    }

    /// The picked vertices and whether they close, for a
    /// [`Self::Perimeter`]; `None` for every other kind.
    ///
    /// # Why both facts come out of ONE accessor
    ///
    /// A vertex list without its `closed` flag is not enough to draw or to
    /// measure — the closing segment is a segment. Two accessors would let a
    /// caller read half the geometry and be confident about it, which is how
    /// a shell ends up drawing an open path over a closed measurement. The
    /// shell needs this for hit-testing and for drawing its vertex handles.
    #[must_use]
    pub fn polyline(&self) -> Option<(&[Point], bool)> {
        match self {
            Self::Perimeter { points, closed, .. } => Some((points.as_slice(), *closed)),
            _ => None,
        }
    }

    /// The mean of a [`Self::Perimeter`]'s vertices — the point its label is
    /// placed relative to. `None` for every other kind, and for an empty
    /// vertex list (which has no mean, rather than a mean of zero).
    ///
    /// The VERTEX mean, deliberately, not the polygon's area centroid: the
    /// vertex mean is defined for an open path and for a self-intersecting
    /// one, and both are shapes this kind accepts. An area centroid is
    /// undefined for the first and signed-degenerate for the second, so the
    /// label would jump or vanish on shapes the measurement itself handles
    /// perfectly well.
    #[must_use]
    pub fn polyline_centroid(&self) -> Option<Point> {
        let (points, _) = self.polyline()?;
        if points.is_empty() {
            return None;
        }
        let n = points.len() as f64;
        let (sx, sy) = points
            .iter()
            .fold((0.0, 0.0), |(sx, sy), p| (sx + p.x, sy + p.y));
        Some(Point::new(sx / n, sy / n))
    }

    /// Whether this ce dimension measures an ANGLE rather than a length.
    ///
    /// The predicate every display and formatting path must consult before
    /// applying a group scale. Provided as a named question rather than
    /// leaving callers to match on the variant, so adding a future angular
    /// kind cannot quietly miss one of them.
    #[must_use]
    pub const fn is_angular(&self) -> bool {
        matches!(self, Self::Angular { .. })
    }

    /// A short label prefix a circular dimension's caption carries (`R`/`⌀`);
    /// empty for a linear dimension. ASCII-safe (`DIA` where a symbol font is
    /// not guaranteed) — the caption generator prepends it.
    #[must_use]
    pub const fn caption_prefix(&self) -> &'static str {
        match self {
            DimensionKind::Linear { .. } => "",
            // No prefix: the caption carries a trailing degree sign instead,
            // which is where a reader expects to find it on an angle.
            DimensionKind::Angular { .. } => "",
            // No prefix either, and this one is a decision rather than an
            // absence. A perimeter's number is a length in the group's unit,
            // identical in kind to a linear one, so a marker such as `P` or
            // `⌀`'s analogue would be pdfcer inventing a notation that neither
            // ASME Y14.2 nor ISO 129-1 defines. The shape drawn beside the
            // number already says what was measured.
            DimensionKind::Perimeter { .. } => "",
            DimensionKind::Circular { show_diameter, .. } => {
                if *show_diameter {
                    "DIA "
                } else {
                    "R "
                }
            }
        }
    }
}

/// One dimension in the model: its immutable geometry, its group, and its
/// in-document wiring handles (the annotation + appearance object numbers,
/// filled in once saved — so a scale change can find and regenerate each
/// member's baked `/AP`).
///
/// **Not `Copy` since `Pass 107.0`**, for the one reason
/// [`DimensionKind`]'s own note gives: it contains a [`DimensionKind`], and a
/// perimeter's vertex list is not bit-copyable.
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionRecord {
    /// Stable identifier.
    pub id: DimensionId,
    /// The group this dimension belongs to.
    pub group: GroupId,
    /// The immutable geometry.
    pub kind: DimensionKind,
    /// The authored annotation object (`None` until wired into a document).
    pub annot: Option<ObjId>,
    /// The authored `/AP` `/N` appearance-stream object (`None` until wired).
    pub ap: Option<ObjId>,
    /// **This ce dimension's own style overrides** (`Pass 69.0`) — the bottom
    /// tier of the cascade ([`super::style`]). `Default::default()` (every
    /// field `None`) means "inherit everything from the group", which is what
    /// every ce dimension authored before this Pass carries.
    ///
    /// On [`DimensionRecord`] rather than on [`DimensionKind`], deliberately
    /// and for the same reason the ui-spec gives for tolerance: `DimensionKind`
    /// is documented in-source as *the immutable geometry*, and a style is
    /// presentation layered on top of a measurement — exactly parallel to how
    /// [`Group`] layers a [`NumberFormat`] over `measured_points`.
    pub style: super::style::StyleOverrides,
    /// **The operator's text override** (`Pass 175.0`, decision 097) — the
    /// caption this ce dimension PRINTS while the override is in force, or
    /// `None` when it prints its measurement.
    ///
    /// # This SHADOWS the measurement; it never replaces it
    ///
    /// Decision 097 is explicit and the distinction is the whole point of the
    /// feature: [`Self::kind`] keeps the measured geometry untouched, the
    /// group keeps its scale and format, and clearing this field back to
    /// `None` restores the measured caption **exactly, with no
    /// re-measurement**. The operator asked for precisely that property —
    /// *"an option if it can be selected to be overridden or not so the
    /// override can be undone"* — which is a requirement about the
    /// reversibility of the OVERRIDE, not about the undo stack (an override
    /// that were merely undoable would be reversible only until the next
    /// edit, like every other command).
    ///
    /// # Why it lives on the record and not in [`StyleOverrides`]
    ///
    /// [`StyleOverrides`] is a CASCADE tier: every field there means "this ce
    /// dimension's answer, or inherit the group's". A text override has no
    /// group tier and must never acquire one — a group-wide "all these
    /// dimensions read 55 5/8" is not a thing any drafting standard has, and
    /// modelling it as inheritable would make one operator keystroke silently
    /// re-caption a whole drawing. It is a per-ce-dimension fact or it is
    /// nothing.
    ///
    /// # `<DIM>` is substituted, so the measurement can stay visible
    ///
    /// The stored string may contain the token `<DIM>`, which
    /// [`super::author_dimension_with_label`] replaces with the measured
    /// caption (prefix + value + tolerance). `2X <DIM> TYP` therefore keeps
    /// tracking the geometry while carrying the operator's annotation — see
    /// that function for the full contract and for why a bare override
    /// replaces the caption WHOLE.
    ///
    /// # Round-trip
    ///
    /// Persisted as the sidecar's `/LabelOverride` key (see
    /// [`super::sidecar`]), so the override survives save-and-reopen — which
    /// decision 097 requires by name, because an override that lived only in
    /// session state would silently revert on the next open and the operator
    /// would have no way to tell an expired override from one that never took.
    pub label_override: Option<String>,
}

/// The authoritative dimensioning model — the whole `/PieceInfo` sidecar's
/// logical content (decision 011 §2.4). Groups + dimensions + the id
/// counters. Serialised by [`super::sidecar`]; never depends on Vec order for
/// identity (uses [`GroupId`]/[`DimensionId`]).
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionModel {
    groups: Vec<Group>,
    dimensions: Vec<DimensionRecord>,
    next_group: u32,
    next_dim: u32,
}

impl Default for DimensionModel {
    fn default() -> Self {
        Self::new()
    }
}

impl DimensionModel {
    /// A fresh model with the single always-present default group (ui-spec
    /// §5.3): "Default", millimetre unit, scale never-set, layer visible.
    #[must_use]
    pub fn new() -> Self {
        let default = Group::new(DEFAULT_GROUP_ID, "Default", Unit::Millimeter);
        Self {
            groups: vec![default],
            dimensions: Vec::new(),
            next_group: 1,
            next_dim: 0,
        }
    }

    /// An empty model with **no** groups — the reconstruction seed for the
    /// sidecar deserializer, which then re-inserts every stored group
    /// (including the default) with its exact id and fields. Not a normal
    /// entry point (a live model always has the default group — use
    /// [`Self::new`]).
    #[must_use]
    pub(crate) fn empty() -> Self {
        Self {
            groups: Vec::new(),
            dimensions: Vec::new(),
            next_group: 0,
            next_dim: 0,
        }
    }

    /// Every group, in insertion order (the default group first).
    #[must_use]
    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// Every dimension, in insertion order.
    #[must_use]
    pub fn dimensions(&self) -> &[DimensionRecord] {
        &self.dimensions
    }

    /// The group with `id`, or `None`.
    #[must_use]
    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Mutable access to the group with `id`, or `None`.
    pub fn group_mut(&mut self, id: GroupId) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// The dimension with `id`, or `None`.
    #[must_use]
    pub fn dimension(&self, id: DimensionId) -> Option<&DimensionRecord> {
        self.dimensions.iter().find(|d| d.id == id)
    }

    /// Mutable access to the dimension with `id`.
    pub fn dimension_mut(&mut self, id: DimensionId) -> Option<&mut DimensionRecord> {
        self.dimensions.iter_mut().find(|d| d.id == id)
    }

    /// Create a new named group with `unit`'s default format, returning its
    /// fresh id. The group starts scale-never-set (ui-spec §4.3) and visible.
    pub fn add_group(&mut self, name: impl Into<String>, unit: Unit) -> GroupId {
        let id = GroupId(self.next_group);
        self.next_group += 1;
        self.groups.push(Group::new(id, name, unit));
        id
    }

    /// Remove the group with `id`, returning whether it existed.
    ///
    /// **Does not touch its members.** Callers decide what happens to them
    /// — see `EditSession::delete_dimension_group_with` and `GroupDeletion`,
    /// where that decision is the operator's rather than this model's. A
    /// remover that silently orphaned dimensions would leave records whose
    /// `group` resolves to nothing, and every reader of this model would
    /// then need a "what if the group is missing" branch.
    pub fn remove_group(&mut self, id: GroupId) -> bool {
        let before = self.groups.len();
        self.groups.retain(|g| g.id != id);
        self.groups.len() != before
    }

    /// Insert a group with a caller-supplied id (used by the sidecar
    /// deserializer to reconstruct exact ids). Keeps `next_group` ahead of any
    /// inserted id. Does nothing if the id already exists.
    pub(crate) fn insert_group(&mut self, group: Group) {
        if self.groups.iter().any(|g| g.id == group.id) {
            return;
        }
        self.next_group = self.next_group.max(group.id.0 + 1);
        self.groups.push(group);
    }

    /// Set a group's scale state and number format in one step (the
    /// scale-entry / group-panel commit). No-op for an unknown group.
    pub fn set_group_scale(&mut self, id: GroupId, scale: ScaleState, format: NumberFormat) {
        if let Some(g) = self.group_mut(id) {
            g.scale = scale;
            g.format = format;
        }
    }

    /// Toggle (or set) a group's default layer visibility. The default group
    /// is un-hideable (ui-spec §5.3): a request to hide it is ignored.
    /// Returns the resulting visibility.
    pub fn set_group_visible(&mut self, id: GroupId, visible: bool) -> bool {
        if id == DEFAULT_GROUP_ID && !visible {
            return true; // un-hideable
        }
        if let Some(g) = self.group_mut(id) {
            g.visible = visible;
            g.visible
        } else {
            true
        }
    }

    /// Add a dimension to `group` (falling back to the default group if the id
    /// is unknown), returning its fresh id. Wiring handles start `None`.
    pub fn add_dimension(&mut self, group: GroupId, kind: DimensionKind) -> DimensionId {
        let group = if self.group(group).is_some() {
            group
        } else {
            DEFAULT_GROUP_ID
        };
        let id = DimensionId(self.next_dim);
        self.next_dim += 1;
        self.dimensions.push(DimensionRecord {
            id,
            group,
            kind,
            annot: None,
            ap: None,
            // A newly authored ce dimension overrides NOTHING: it takes its
            // whole appearance from its group. Anything else would make the
            // group's default a suggestion rather than a default.
            style: super::style::StyleOverrides::default(),
            // A newly authored ce dimension prints its MEASUREMENT. An
            // override is something the operator asks for afterwards, on a
            // dimension they can already see — there is no defensible way to
            // guess one at draw time (decision 097).
            label_override: None,
        });
        id
    }

    /// Insert a dimension with a caller-supplied record (sidecar
    /// deserializer). Keeps `next_dim` ahead of the inserted id.
    pub(crate) fn insert_dimension(&mut self, record: DimensionRecord) {
        if self.dimensions.iter().any(|d| d.id == record.id) {
            return;
        }
        self.next_dim = self.next_dim.max(record.id.0 + 1);
        self.dimensions.push(record);
    }

    /// The dimensions belonging to `group`, in insertion order.
    pub fn members(&self, group: GroupId) -> impl Iterator<Item = &DimensionRecord> + '_ {
        self.dimensions.iter().filter(move |d| d.group == group)
    }

    /// How many dimensions belong to `group` (the count the group panel shows).
    #[must_use]
    pub fn member_count(&self, group: GroupId) -> usize {
        self.members(group).count()
    }

    /// The displayed value of a dimension under its group's scale + format —
    /// the derived value (decision 011 §2.3). `None` for an unknown dimension.
    #[must_use]
    pub fn display(&self, id: DimensionId) -> Option<MeasurementDisplay> {
        let d = self.dimension(id)?;
        let g = self.group(d.group)?;
        Some(d.kind.display_with(g.scale, g.format))
    }

    /// The text override in force on `id`, or `None` when that ce dimension
    /// prints its measurement (`Pass 175.0`, decision 097).
    ///
    /// # Read this ALONGSIDE [`Self::display`], never instead of it
    ///
    /// [`Self::display`] deliberately keeps answering the MEASURED value even
    /// while an override is in force, because that is the question it has
    /// always answered and a surface that needs the measurement (a re-measure
    /// check, a scale preview, an export of what the drawing actually says it
    /// measured) still needs it. The caption that PRINTS is a different
    /// question, answered by [`super::author_dimension_with_label`].
    ///
    /// A surface showing a ce dimension to an operator owes BOTH: rule 4 asks
    /// that a caption diverging from its measurement be disclosed, and the
    /// only way to disclose a divergence is to show the two things that
    /// diverge. `pdfcer`'s `dimension-list` prints `override=…` beside the
    /// measured `value=…` for exactly this reason.
    #[must_use]
    pub fn label_override(&self, id: DimensionId) -> Option<&str> {
        self.dimension(id)?.label_override.as_deref()
    }

    /// Delete `group`, reassigning its members to the default group (ui-spec
    /// §5.2: never a silent orphan). The default group cannot be deleted.
    /// Returns how many members were reassigned.
    /// Remove one dimension from the model, returning whether it was there.
    ///
    /// The record only — the caller owns removing the annotation objects it
    /// names. Keeping those two halves in one method would put document
    /// mutation inside the pure model, which is what keeps this type testable
    /// without a `Document` at all.
    ///
    /// The dimension's GROUP is deliberately left, even when this was its last
    /// member: a group is a named container carrying a scale the operator
    /// calibrated, and discarding that as a side effect of removing the last
    /// dimension would silently throw away work that is not cheap to redo.
    pub fn remove_dimension(&mut self, id: DimensionId) -> bool {
        let before = self.dimensions.len();
        self.dimensions.retain(|d| d.id != id);
        self.dimensions.len() != before
    }

    pub fn delete_group(&mut self, id: GroupId) -> usize {
        if id == DEFAULT_GROUP_ID {
            return 0;
        }
        let mut moved = 0;
        for d in &mut self.dimensions {
            if d.group == id {
                d.group = DEFAULT_GROUP_ID;
                moved += 1;
            }
        }
        self.groups.retain(|g| g.id != id);
        moved
    }
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
    use crate::dimension::units::ScaleState;

    fn linear(a: (f64, f64), b: (f64, f64), c: AxisConstraint) -> DimensionKind {
        DimensionKind::Linear {
            a: Point::new(a.0, a.1),
            b: Point::new(b.0, b.1),
            constraint: c,
            offset: 0.0,
            text_along: 0.0,
        }
    }

    #[test]
    fn a_fresh_model_has_exactly_the_default_group() {
        let m = DimensionModel::new();
        assert_eq!(m.groups().len(), 1);
        assert_eq!(m.groups()[0].id, DEFAULT_GROUP_ID);
        assert_eq!(m.groups()[0].name, "Default");
        assert!(m.groups()[0].scale.is_never_set());
    }

    #[test]
    fn measured_points_linear_and_circular() {
        let horiz = linear((10.0, 20.0), (50.0, 80.0), AxisConstraint::Horizontal);
        assert_eq!(horiz.measured_points(), 40.0);
        let aligned = linear((0.0, 0.0), (3.0, 4.0), AxisConstraint::Aligned);
        assert_eq!(aligned.measured_points(), 5.0);
        let circ = DimensionKind::Circular {
            fit: FitCircle {
                center: Point::new(0.0, 0.0),
                radius: 10.0,
                residual: 0.0,
            },
            show_diameter: false,
        };
        assert_eq!(circ.measured_points(), 10.0);
        let dia = DimensionKind::Circular {
            fit: FitCircle {
                center: Point::new(0.0, 0.0),
                radius: 10.0,
                residual: 0.0,
            },
            show_diameter: true,
        };
        assert_eq!(dia.measured_points(), 20.0);
        assert_eq!(dia.caption_prefix(), "DIA ");
    }

    #[test]
    fn scale_change_re_propagates_to_every_member() {
        // The decision-011 headline: change the group scale → all members
        // update. Two dimensions in one group; change the scale once; both
        // displayed values change.
        let mut m = DimensionModel::new();
        let g = m.add_group("Floor Plan", Unit::Meter);
        let d1 = m.add_dimension(
            g,
            linear((0.0, 0.0), (100.0, 0.0), AxisConstraint::Horizontal),
        );
        let d2 = m.add_dimension(
            g,
            linear((0.0, 0.0), (200.0, 0.0), AxisConstraint::Horizontal),
        );

        // Never-set → raw page units.
        assert!(m.display(d1).unwrap().raw_page_units);

        // Calibrate: 1 pt = 0.01 m.
        m.set_group_scale(
            g,
            ScaleState::Calibrated { scale: 0.01 },
            NumberFormat::decimal(Unit::Meter, 2),
        );
        assert_eq!(m.display(d1).unwrap().text, "1.00 m");
        assert_eq!(m.display(d2).unwrap().text, "2.00 m");

        // Re-scale once → BOTH members update.
        m.set_group_scale(
            g,
            ScaleState::Calibrated { scale: 0.05 },
            NumberFormat::decimal(Unit::Meter, 2),
        );
        assert_eq!(m.display(d1).unwrap().text, "5.00 m");
        assert_eq!(m.display(d2).unwrap().text, "10.00 m");
    }

    #[test]
    fn deleting_a_group_reassigns_members_never_orphans() {
        let mut m = DimensionModel::new();
        let g = m.add_group("Temp", Unit::Millimeter);
        let d = m.add_dimension(g, linear((0.0, 0.0), (10.0, 0.0), AxisConstraint::Aligned));
        assert_eq!(m.member_count(g), 1);
        let moved = m.delete_group(g);
        assert_eq!(moved, 1);
        assert!(m.group(g).is_none());
        // The dimension survived, now in the default group.
        assert_eq!(m.dimension(d).unwrap().group, DEFAULT_GROUP_ID);
        assert_eq!(m.member_count(DEFAULT_GROUP_ID), 1);
    }

    #[test]
    fn default_group_is_un_hideable_and_undeletable() {
        let mut m = DimensionModel::new();
        assert!(m.set_group_visible(DEFAULT_GROUP_ID, false));
        assert!(m.group(DEFAULT_GROUP_ID).unwrap().visible);
        assert_eq!(m.delete_group(DEFAULT_GROUP_ID), 0);
        assert!(m.group(DEFAULT_GROUP_ID).is_some());
    }

    #[test]
    fn adding_to_an_unknown_group_falls_back_to_default() {
        let mut m = DimensionModel::new();
        let d = m.add_dimension(
            GroupId(999),
            linear((0.0, 0.0), (1.0, 0.0), AxisConstraint::Aligned),
        );
        assert_eq!(m.dimension(d).unwrap().group, DEFAULT_GROUP_ID);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod angular_tests {
    use super::*;
    use crate::dimension::units::ScaleState;

    fn wedge(degrees: f64) -> DimensionKind {
        let r = degrees.to_radians();
        DimensionKind::Angular {
            apex: Point::new(100.0, 100.0),
            dir_a: Point::new(1.0, 0.0),
            dir_b: Point::new(r.cos(), r.sin()),
            radius: 40.0,
            text_along: 0.0,
        }
    }

    #[test]
    fn the_measured_value_is_the_angle_between_the_arms() {
        for expected in [15.0, 30.0, 45.0, 90.0, 135.0] {
            let got = wedge(expected).measured_points();
            assert!(
                (got - expected).abs() < 1e-9,
                "expected {expected} degrees, got {got}"
            );
        }
    }

    /// ★ An angle must NOT be multiplied by the group's scale.
    ///
    /// This is the one place the ce-dimension value model genuinely differs
    /// from every other kind, and getting it wrong is not a visible crash: a
    /// 30-degree angle in a 1:50 group would render as 1500 with no unit and
    /// no disclosure that anything was odd. It would simply be a wrong number
    /// that looked like a measurement.
    #[test]
    fn an_angle_is_not_scaled_by_the_group() {
        let mut model = DimensionModel::new();
        let g = model.add_group("Plan", Unit::Millimeter);
        model.set_group_scale(
            g,
            ScaleState::Calibrated { scale: 50.0 },
            Unit::Millimeter.default_format(),
        );
        let id = model.add_dimension(g, wedge(30.0));

        let shown = model.display(id).expect("a display value");
        assert!(
            shown.text.starts_with("30"),
            "a 30-degree angle in a 1:50 group must still read 30, got {:?}",
            shown.text
        );
        assert!(
            shown.text.contains('\u{b0}'),
            "and must carry a degree sign, got {:?}",
            shown.text
        );
        assert!(
            !shown.text.contains("mm"),
            "an angle is not in millimetres, got {:?}",
            shown.text
        );
    }

    /// The same wedge under a NEVER-SET scale still reads as an angle.
    ///
    /// The raw-page-units disclosure exists because an unscaled LENGTH is
    /// meaningless to an operator. An angle is meaningful without a scale, so
    /// claiming raw page units would be a false warning.
    #[test]
    fn an_angle_needs_no_scale_disclosure() {
        let mut model = DimensionModel::new();
        let id = model.add_dimension(DEFAULT_GROUP_ID, wedge(45.0));
        let shown = model.display(id).expect("a display value");
        assert!(
            !shown.raw_page_units,
            "an angle is not raw page units; it is the same 45 degrees at any scale"
        );
        assert!(shown.text.starts_with("45"), "got {:?}", shown.text);
    }

    /// Moving an angular ce dimension moves its apex and nothing else.
    ///
    /// The arm directions are UNIT VECTORS. Translating them would rotate the
    /// dimension — silently changing the angle it measures, which is the one
    /// thing a move must never do.
    #[test]
    fn moving_an_angle_does_not_change_it() {
        let before = wedge(37.5);
        let after = before.clone().translated(250.0, -80.0);
        assert!(
            (after.measured_points() - before.measured_points()).abs() < 1e-12,
            "a translation must preserve the measured angle"
        );
        match after {
            DimensionKind::Angular { apex, .. } => {
                assert!((apex.x - 350.0).abs() < 1e-9 && (apex.y - 20.0).abs() < 1e-9);
            }
            other => panic!("expected Angular, got {other:?}"),
        }
    }
}

/// Transform a [`DimensionKind`]'s geometry by `m` (`Pass 120.4`).
///
/// Every variant is point-based, so a transform is exact — there is no
/// rectangle here to lose to a rotation, which is the one place the markup
/// twin has to compromise.
///
/// # What is deliberately NOT transformed
///
/// `offset` and `text_along` are **placement scalars in the dimension's own
/// frame**, not page-space points. `radius` on an angular dimension is the arc
/// radius in the same frame. Scaling them by the matrix's magnitude would be
/// right for a uniform scale and wrong for everything else, and a pasted
/// dimension whose label has drifted off its own leader is worse than one
/// whose label offset stayed put. A **measurement is re-derived from the
/// geometry on authoring**, so the number stays correct either way.
#[must_use]
pub fn transform_kind(kind: &DimensionKind, m: crate::vector::Matrix) -> DimensionKind {
    let pt = |p: &Point| m.map_point(*p);
    // No wildcard arm: a new dimension variant must decide how it transforms
    // at compile time rather than shipping as an un-moved copy.
    match kind {
        DimensionKind::Linear {
            a,
            b,
            constraint,
            offset,
            text_along,
        } => DimensionKind::Linear {
            a: pt(a),
            b: pt(b),
            constraint: *constraint,
            offset: *offset,
            text_along: *text_along,
        },
        DimensionKind::Circular { fit, show_diameter } => DimensionKind::Circular {
            fit: crate::dimension::FitCircle {
                center: pt(&fit.center),
                // The radius scales with the geometry, unlike the placement
                // scalars above: it IS geometry -- the circle's own size.
                radius: fit.radius * m.determinant().abs().sqrt(),
                ..*fit
            },
            show_diameter: *show_diameter,
        },
        DimensionKind::Angular {
            apex,
            dir_a,
            dir_b,
            radius,
            text_along,
        } => DimensionKind::Angular {
            apex: pt(apex),
            dir_a: pt(dir_a),
            dir_b: pt(dir_b),
            radius: *radius,
            text_along: *text_along,
        },
        DimensionKind::Perimeter {
            points,
            closed,
            offset,
            text_along,
        } => DimensionKind::Perimeter {
            points: points.iter().map(pt).collect(),
            closed: *closed,
            offset: *offset,
            text_along: *text_along,
        },
    }
}

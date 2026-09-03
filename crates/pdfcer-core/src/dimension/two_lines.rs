//! Turning two picked lines into an authored **ce dimension** (`Pass 68.0`).
//!
//! # What this module is for
//!
//! [`crate::vector::linepick`] answers the *geometric* question — are these
//! two lines parallel, collinear, or angled, and if angled, which of the four
//! angles did the operator point at. It deliberately stops there: it knows
//! nothing about ce dimensions, and returns a [`TwoLineRelation`] rather than
//! anything drawable.
//!
//! This module is the step after: it turns that relation into the actual
//! [`DimensionKind`] that `EditSession::add_dimension` stores. That involves a
//! set of decisions no geometry function should be making on its own — which
//! way the linear dimension's second point is signed, what arc radius an
//! angular dimension defaults to, and which relations are refused outright.
//!
//! # ★ Why this lives in core rather than in each shell
//!
//! It was written twice-shaped and shipped once: `pdfcer`'s
//! `dimension-add --kind two-lines` carried the whole mapping inline. When the
//! GUI gesture came to be built, the obvious move was to copy those eighty
//! lines onto the canvas and pin them together with an equivalence test, the
//! way [`crate::dimension::group::DimensionKind::Linear`]'s simpler
//! construction is pinned in `pdfce-gui`'s `measure_tool` module.
//!
//! That would have been the wrong instrument for this one. An equivalence test
//! pins two implementations to *agree today*; it does not stop them being
//! edited apart tomorrow, and it fails only if someone remembers to extend it
//! when a third parameter appears. The mapping here is not a two-line
//! constructor — it carries a sign convention (which side of line A the
//! dimension runs toward), a fallback radius, and two distinct refusals. Two
//! copies of that is how the CLI and the GUI come to author *visibly
//! different* ce dimensions from the same two clicks.
//!
//! `Settings::parallel_epsilon_degrees` exists for precisely the same reason
//! one rung down — so the two shells cannot come to disagree about when two
//! lines count as parallel. Duplicating the code that consumes it would have
//! reintroduced the disagreement one level above the value that was
//! centralised to prevent it.
//!
//! So there is **one** implementation, both shells call it, and the only thing
//! a shell decides is how to *disclose* what came back.
//!
//! # What the caller still owes the operator
//!
//! Everything here is an inference — pdfcer reading geometry and deciding what
//! the operator meant — so project rule 4 applies in full. [`TwoLineAuthoring`]
//! therefore carries the *evidence* alongside the result:
//! [`TwoLineAuthoring::measured_angle_degrees`] is present even when the
//! parallel reading was forced, so a shell can say "these are 0.8° apart, read
//! as parallel because you asked" rather than silently presenting a distance.
//! A shell that shows the result and hides the measurement is asking the
//! operator to accept a decision while withholding the fact that makes it one.

use super::group::DimensionKind;
use crate::vector::linepick::{
    ParallelPolicy, PickedLine, TwoLineRelation, classify_two_lines, measured_angle_degrees,
};
use crate::vector::{AxisConstraint, Point};

/// Where the authored ce dimension sits, independent of which kind it is.
///
/// Bundled rather than passed as three loose scalars because the two authored
/// kinds consume them differently ([`DimensionKind::Angular`] reads `offset`
/// as an arc radius and ignores `constraint` entirely), and a bundle keeps
/// that asymmetry documented in one place instead of at every call site.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TwoLinePlacement {
    /// Axis constraint for a LINEAR result. Ignored for an angular one — an
    /// angle has no horizontal or vertical reading.
    pub constraint: AxisConstraint,
    /// Standoff for a linear result; **arc radius** for an angular one when
    /// non-zero. Zero means "choose a readable default", which for the angular
    /// case is derived from the arm lengths (see [`author_from_two_lines`]).
    pub offset: f64,
    /// Position of the value text along the dimension line (points) or along
    /// the arc (degrees), per [`DimensionKind`].
    pub text_along: f64,
}

impl Default for TwoLinePlacement {
    /// Neutral placement: aligned, no standoff, centred text.
    ///
    /// The same neutral placement the GUI's own two-pick tools produce, so an
    /// operator who authors from two lines and an operator who types four
    /// coordinates get identical bytes for identical geometry.
    fn default() -> Self {
        Self {
            constraint: AxisConstraint::Aligned,
            offset: 0.0,
            text_along: 0.0,
        }
    }
}

/// Why pdfcer declined to author a ce dimension from a pair of picked lines.
///
/// Both variants are **refusals by name**, not silent no-ops: the caller is
/// expected to tell the operator which one happened. A tool that declines
/// without saying why reads as broken rather than careful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TwoLineRefusal {
    /// The two lines lie on the same infinite line.
    ///
    /// # Why this is refused rather than authored as zero
    ///
    /// The perpendicular distance is zero and there is no angle, so the only
    /// thing that could be authored is a zero-length ce dimension — a mark
    /// that is *present in the document and invisible on the page*. The
    /// operator would go looking for it. Refusing costs one message; authoring
    /// it costs a hunt.
    #[error(
        "those two lines are collinear — they lie on the same line, so the distance \
         between them is zero and there is no angle"
    )]
    Collinear,
    /// One of the two lines has zero length, so it has no direction.
    ///
    /// Every possible answer would be invented from nothing, which is exactly
    /// what [`PickedLine::direction`] returning `Option` exists to prevent.
    #[error("one of those lines has zero length — two distinct points are needed per line")]
    Degenerate,
}

/// A ce dimension authored from two picked lines, with the evidence behind it.
///
/// The extra fields are not diagnostics. They are what a shell needs in order
/// to satisfy project rule 4's disclosure obligation without re-measuring
/// anything itself — re-measuring in the shell is how a disclosure comes to
/// contradict the dimension it describes.
///
/// **Not `Copy` since `Pass 107.0`** — it carries a [`DimensionKind`], and
/// that enum stopped being `Copy` when the perimeter variant arrived with a
/// vertex list. Nothing here changed shape; the derive simply cannot hold.
#[derive(Debug, Clone, PartialEq)]
pub struct TwoLineAuthoring {
    /// The ce dimension to hand to `EditSession::add_dimension`.
    pub kind: DimensionKind,
    /// What the two lines were read as. Never [`TwoLineRelation::Collinear`]
    /// here — that path returns [`TwoLineRefusal::Collinear`] instead, so a
    /// caller holding a `TwoLineAuthoring` cannot be holding a refusal.
    pub relation: TwoLineRelation,
    /// The **true** angle between the two lines, folded into `[0, 90]`.
    ///
    /// Reported even when [`Self::forced_parallel`] is set — especially then.
    /// This is the number the override overrode, and a checkbox that hides it
    /// is withholding the fact that makes the decision a decision.
    pub measured_angle_degrees: Option<f64>,
    /// Whether the parallel reading came from the operator's explicit override
    /// rather than from the measurement falling inside the threshold.
    pub forced_parallel: bool,
}

impl TwoLineAuthoring {
    /// Whether a LINEAR ce dimension was authored (as opposed to an angular
    /// one). Convenience for shells choosing which disclosure line to show.
    #[must_use]
    pub const fn is_linear(&self) -> bool {
        matches!(self.relation, TwoLineRelation::Parallel { .. })
    }

    /// For an angular result, whether the two lines actually meet.
    ///
    /// `Some(false)` means the apex is **virtual** — the lines would only
    /// cross if extended. That is a perfectly ordinary thing to dimension in a
    /// CAD drawing, so it is not refused; but it is a fact about the geometry
    /// the operator may not have noticed, so a shell is expected to say it.
    /// `None` for a linear result, which has no apex.
    #[must_use]
    pub const fn apex_is_real(&self) -> Option<bool> {
        match self.relation {
            TwoLineRelation::Angled { apex_is_real, .. } => Some(apex_is_real),
            _ => None,
        }
    }
}

/// The arc radius used for an angular ce dimension when the caller asks for no
/// specific standoff, as a fraction of the shorter arm.
///
/// Half the shorter arm puts the arc *on* the geometry being measured rather
/// than at some absolute distance from it, which is what keeps a 20-point
/// detail and a 2000-point assembly both legible without the operator
/// adjusting anything.
const DEFAULT_ARC_FRACTION: f64 = 0.5;

/// The floor on that default, in points.
///
/// Without it, two very short arms produce an arc too small to see or click,
/// and the ce dimension is technically present and practically unusable — the
/// same failure the collinear refusal exists to avoid, arrived at by a
/// different route.
const MIN_ARC_RADIUS: f64 = 20.0;

/// Read two picked lines and author the ce dimension they call for.
///
/// This is the single implementation behind both `pdfcer`'s
/// `dimension-add --kind two-lines` and the GUI's two-line gesture — see the
/// module docs for why it is not duplicated per shell.
///
/// # What gets authored
///
/// | relation | result |
/// |---|---|
/// | parallel (or forced) | [`DimensionKind::Linear`] of the perpendicular distance |
/// | angled | [`DimensionKind::Angular`] of the angle the operator pointed at |
/// | collinear | [`TwoLineRefusal::Collinear`] |
/// | either line degenerate | [`TwoLineRefusal::Degenerate`] |
///
/// # The linear case runs *across* the gap, not along a line
///
/// The authored `a` is the first line's pick point and `b` is that point
/// displaced perpendicular to line A by the measured distance — so the ce
/// dimension spans the gap it is reporting. The normal is signed toward the
/// *other* line, because the unsigned normal points to whichever side the
/// winding happened to produce, and half the time that is away from the thing
/// being measured. An operator would see the dimension drawn on the wrong side
/// of the edge, with the right number on it.
///
/// # Errors
///
/// Returns [`TwoLineRefusal`] when the pair cannot yield a meaningful ce
/// dimension — collinear lines, or a zero-length one. Both are conditions the
/// caller is expected to surface to the operator by name.
///
/// # Examples
///
/// Two parallel edges 40 points apart author a linear ce dimension:
///
/// ```
/// use pdfcer_core::dimension::{author_from_two_lines, TwoLinePlacement};
/// use pdfcer_core::vector::Point;
/// use pdfcer_core::vector::linepick::{ParallelPolicy, PickedLine};
/// use pdfcer_core::vector::HitTarget;
///
/// let mk = |sx, sy, ex, ey| PickedLine {
///     target: HitTarget::Object(0),
///     subpath: 0,
///     segment: 0,
///     start: Point::new(sx, sy),
///     end: Point::new(ex, ey),
///     pick: Point::new(f64::midpoint(sx, ex), f64::midpoint(sy, ey)),
/// };
/// let authored = author_from_two_lines(
///     &mk(100.0, 100.0, 300.0, 100.0),
///     &mk(100.0, 140.0, 300.0, 140.0),
///     ParallelPolicy::from_setting(0.5),
///     TwoLinePlacement::default(),
/// )?;
/// assert!(authored.is_linear());
/// # Ok::<(), pdfcer_core::dimension::TwoLineRefusal>(())
/// ```
pub fn author_from_two_lines(
    a: &PickedLine,
    b: &PickedLine,
    policy: ParallelPolicy,
    placement: TwoLinePlacement,
) -> Result<TwoLineAuthoring, TwoLineRefusal> {
    let relation = classify_two_lines(a, b, policy).ok_or(TwoLineRefusal::Degenerate)?;
    let measured = measured_angle_degrees(a, b);

    let kind = match relation {
        TwoLineRelation::Collinear => return Err(TwoLineRefusal::Collinear),
        TwoLineRelation::Parallel { distance } => {
            // `direction()` cannot be `None` here: `classify_two_lines` already
            // returned `Some`, which required both directions. The fallback is
            // unreachable rather than meaningful, and is written as the +x axis
            // only because some value is syntactically required.
            let (ux, uy) = a.direction().unwrap_or((1.0, 0.0));
            let (nx, ny) = (-uy, ux);
            // Sign the normal toward the OTHER line, so the ce dimension spans
            // the gap instead of pointing away from it (see the doc comment).
            let toward = (b.pick.x - a.pick.x).mul_add(nx, (b.pick.y - a.pick.y) * ny);
            let sign = if toward < 0.0 { -1.0 } else { 1.0 };
            DimensionKind::Linear {
                a: a.pick,
                b: Point::new(
                    (nx * sign).mul_add(distance, a.pick.x),
                    (ny * sign).mul_add(distance, a.pick.y),
                ),
                constraint: placement.constraint,
                offset: placement.offset,
                text_along: placement.text_along,
            }
        }
        TwoLineRelation::Angled { apex, .. } => {
            // Each arm points from the apex toward where the operator clicked
            // — the same rule that chose which of the four angles this is, so
            // the drawn wedge and the measured value cannot disagree.
            let dir = |p: &PickedLine| {
                let (dx, dy) = (p.pick.x - apex.x, p.pick.y - apex.y);
                let len = dx.hypot(dy);
                if len <= f64::EPSILON {
                    Point::new(1.0, 0.0)
                } else {
                    Point::new(dx / len, dy / len)
                }
            };
            DimensionKind::Angular {
                apex,
                dir_a: dir(a),
                dir_b: dir(b),
                radius: if placement.offset.abs() > f64::EPSILON {
                    placement.offset.abs()
                } else {
                    (a.length().min(b.length()) * DEFAULT_ARC_FRACTION).max(MIN_ARC_RADIUS)
                },
                text_along: placement.text_along,
            }
        }
    };

    Ok(TwoLineAuthoring {
        kind,
        relation,
        measured_angle_degrees: measured,
        forced_parallel: policy.force_parallel,
    })
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

    /// A picked line whose pick point is its midpoint — the CLI's own default,
    /// and the neutral choice when a test is not exercising the four-angle
    /// disambiguation.
    fn line(sx: f64, sy: f64, ex: f64, ey: f64) -> PickedLine {
        PickedLine {
            target: crate::vector::HitTarget::Object(0),
            subpath: 0,
            segment: 0,
            start: Point::new(sx, sy),
            end: Point::new(ex, ey),
            pick: Point::new(f64::midpoint(sx, ex), f64::midpoint(sy, ey)),
        }
    }

    /// A picked line with an explicit pick point, for the angle-selection case.
    fn line_picked(sx: f64, sy: f64, ex: f64, ey: f64, px: f64, py: f64) -> PickedLine {
        PickedLine {
            pick: Point::new(px, py),
            ..line(sx, sy, ex, ey)
        }
    }

    fn default_policy() -> ParallelPolicy {
        ParallelPolicy::from_setting(0.5)
    }

    /// ★ The parallel case, pinned against the exact numbers the shipped CLI
    /// prints for these inputs (`distance=40.0000`).
    #[test]
    fn parallel_lines_author_a_linear_ce_dimension_of_the_perpendicular_distance() {
        let authored = author_from_two_lines(
            &line(100.0, 100.0, 300.0, 100.0),
            &line(100.0, 140.0, 300.0, 140.0),
            default_policy(),
            TwoLinePlacement::default(),
        )
        .expect("two parallel edges are dimensionable");

        assert!(authored.is_linear());
        match authored.kind {
            DimensionKind::Linear { a, b, .. } => {
                assert!((a.x - 200.0).abs() < 1e-9, "anchored at line A's pick");
                assert!((a.y - 100.0).abs() < 1e-9);
                // Perpendicular, toward the other line, exactly 40 away.
                assert!((b.x - 200.0).abs() < 1e-9, "runs perpendicular, got {b:?}");
                assert!((b.y - 140.0).abs() < 1e-9, "spans the gap, got {b:?}");
            }
            other => panic!("expected Linear, got {other:?}"),
        }
    }

    /// ★ The sign convention. With line B *below* line A the ce dimension must
    /// still span the gap, not point away from it — the failure this guards is
    /// a dimension drawn on the wrong side of the edge with the right number
    /// on it, which looks like a rendering bug rather than a sign bug.
    #[test]
    fn the_linear_normal_is_signed_toward_the_other_line() {
        let authored = author_from_two_lines(
            &line(100.0, 140.0, 300.0, 140.0),
            &line(100.0, 100.0, 300.0, 100.0),
            default_policy(),
            TwoLinePlacement::default(),
        )
        .expect("dimensionable");
        match authored.kind {
            DimensionKind::Linear { a, b, .. } => {
                assert!((a.y - 140.0).abs() < 1e-9);
                assert!(
                    (b.y - 100.0).abs() < 1e-9,
                    "must reach DOWN to the other line, got {b:?}"
                );
            }
            other => panic!("expected Linear, got {other:?}"),
        }
    }

    /// ★ The angular case, pinned against the CLI's printed `degrees=30.029`
    /// for the same four coordinates.
    #[test]
    fn angled_lines_author_an_angular_ce_dimension() {
        let authored = author_from_two_lines(
            &line(100.0, 100.0, 300.0, 100.0),
            &line(100.0, 100.0, 273.0, 200.0),
            default_policy(),
            TwoLinePlacement::default(),
        )
        .expect("two angled edges are dimensionable");

        assert!(!authored.is_linear());
        match authored.relation {
            TwoLineRelation::Angled { degrees, .. } => {
                assert!(
                    (degrees - 30.029).abs() < 0.01,
                    "must match the CLI's own reading, got {degrees}"
                );
            }
            other => panic!("expected Angled, got {other:?}"),
        }
        assert!(matches!(authored.kind, DimensionKind::Angular { .. }));
    }

    /// Collinear is refused BY NAME, not authored as a zero-length mark.
    #[test]
    fn collinear_lines_are_refused() {
        let err = author_from_two_lines(
            &line(0.0, 0.0, 100.0, 0.0),
            &line(200.0, 0.0, 300.0, 0.0),
            default_policy(),
            TwoLinePlacement::default(),
        )
        .expect_err("collinear lines have neither a distance nor an angle");
        assert_eq!(err, TwoLineRefusal::Collinear);
    }

    /// A zero-length line is refused distinctly from collinearity, because the
    /// operator's remedy differs: one means "pick a different pair", the other
    /// means "that click did not land on a line".
    #[test]
    fn a_degenerate_line_is_refused_distinctly() {
        let err = author_from_two_lines(
            &line(10.0, 10.0, 10.0, 10.0),
            &line(0.0, 0.0, 100.0, 0.0),
            default_policy(),
            TwoLinePlacement::default(),
        )
        .expect_err("a zero-length line has no direction");
        assert_eq!(err, TwoLineRefusal::Degenerate);
    }

    /// ★ The override authors a LINEAR ce dimension **and still reports the
    /// angle it overrode**. A shell cannot disclose what it is not given, so
    /// this is the field that makes the checkbox honest.
    #[test]
    fn forcing_parallel_authors_linear_and_still_reports_the_measured_angle() {
        let t = 5f64.to_radians().tan();
        let a = line(0.0, 0.0, 100.0, 0.0);
        let b = line(0.0, 40.0, 100.0, 100.0f64.mul_add(t, 40.0));

        let auto = author_from_two_lines(&a, &b, default_policy(), TwoLinePlacement::default())
            .expect("dimensionable");
        assert!(!auto.is_linear(), "5 degrees apart reads as angled");

        let forced = author_from_two_lines(
            &a,
            &b,
            default_policy().forcing_parallel(),
            TwoLinePlacement::default(),
        )
        .expect("dimensionable");
        assert!(forced.is_linear(), "the operator's override must win");
        assert!(forced.forced_parallel);
        let measured = forced
            .measured_angle_degrees
            .expect("the overridden angle must still be reported");
        assert!(
            (measured - 5.0).abs() < 0.01,
            "the disclosure must carry the TRUE angle, got {measured}"
        );
    }

    /// A virtual apex is authored and flagged, never refused — CAD drawings
    /// dimension virtual intersections routinely.
    #[test]
    fn a_virtual_apex_is_authored_and_flagged_for_disclosure() {
        let authored = author_from_two_lines(
            &line(0.0, 0.0, 50.0, 0.0),
            &line(100.0, 50.0, 150.0, 100.0),
            default_policy(),
            TwoLinePlacement::default(),
        )
        .expect("two lines that would meet if extended still define an angle");
        assert_eq!(authored.apex_is_real(), Some(false));
    }

    /// A linear result has no apex to report on, and says so rather than
    /// returning a plausible `false`.
    #[test]
    fn a_linear_result_reports_no_apex_at_all() {
        let authored = author_from_two_lines(
            &line(0.0, 0.0, 100.0, 0.0),
            &line(0.0, 40.0, 100.0, 40.0),
            default_policy(),
            TwoLinePlacement::default(),
        )
        .expect("dimensionable");
        assert_eq!(authored.apex_is_real(), None);
    }

    /// ★ The pick point still selects which of the four angles is authored,
    /// through this layer. The whole reason `PickedLine::pick` exists would be
    /// lost if the authoring step collapsed to the smallest angle.
    #[test]
    fn the_pick_point_still_selects_the_angle_after_authoring() {
        let (bx, by) = (60f64.to_radians().cos(), 60f64.to_radians().sin());
        let a_pos = line_picked(-100.0, 0.0, 100.0, 0.0, 50.0, 0.0);
        let a_neg = line_picked(-100.0, 0.0, 100.0, 0.0, -50.0, 0.0);
        let b_pos = line_picked(
            -100.0 * bx,
            -100.0 * by,
            100.0 * bx,
            100.0 * by,
            50.0 * bx,
            50.0 * by,
        );

        let deg = |a: &PickedLine, b: &PickedLine| match author_from_two_lines(
            a,
            b,
            default_policy(),
            TwoLinePlacement::default(),
        )
        .expect("dimensionable")
        .relation
        {
            TwoLineRelation::Angled { degrees, .. } => degrees,
            other => panic!("expected Angled, got {other:?}"),
        };
        assert!((deg(&a_pos, &b_pos) - 60.0).abs() < 1e-6);
        assert!((deg(&a_neg, &b_pos) - 120.0).abs() < 1e-6);
    }

    /// The arc radius falls back to a readable default, floored so two short
    /// arms still produce a clickable arc.
    #[test]
    fn the_default_arc_radius_is_floored_for_short_arms() {
        let authored = author_from_two_lines(
            &line(0.0, 0.0, 4.0, 0.0),
            &line(0.0, 0.0, 0.0, 4.0),
            default_policy(),
            TwoLinePlacement::default(),
        )
        .expect("dimensionable");
        match authored.kind {
            DimensionKind::Angular { radius, .. } => {
                assert!(
                    (radius - MIN_ARC_RADIUS).abs() < 1e-9,
                    "two 4-point arms must still get a visible arc, got {radius}"
                );
            }
            other => panic!("expected Angular, got {other:?}"),
        }
    }

    /// An explicit standoff overrides the derived arc radius, which is how the
    /// operator drags the arc where they want it.
    #[test]
    fn an_explicit_offset_becomes_the_arc_radius() {
        let authored = author_from_two_lines(
            &line(0.0, 0.0, 100.0, 0.0),
            &line(0.0, 0.0, 0.0, 100.0),
            default_policy(),
            TwoLinePlacement {
                offset: 75.0,
                ..TwoLinePlacement::default()
            },
        )
        .expect("dimensionable");
        match authored.kind {
            DimensionKind::Angular { radius, .. } => {
                assert!((radius - 75.0).abs() < 1e-9, "got {radius}");
            }
            other => panic!("expected Angular, got {other:?}"),
        }
    }
}

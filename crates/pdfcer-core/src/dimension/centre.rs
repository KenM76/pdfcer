//! Centre lines and centre marks: ce-dimension construction geometry that
//! measures nothing ([`DimensionKind::CentreLine`],
//! [`DimensionKind::CentreMark`], and the mark a [`DimensionKind::Circular`]
//! can carry).
//!
//! Stored and read back through the same [`DimensionModel`](crate::dimension::group::DimensionModel) as every
//! other ce dimension, so grouping, move, rotate, delete, undo and the
//! `/PieceInfo` sidecar need nothing new.
//!
//! The centre mark follows the drafting convention AutoCAD documents for
//! `DIMCEN` (a behavioural reference): the cross arm reaches `size` from the
//! centre; extension lines start a further `size` out (the gap) and end
//! `size` past the rim.

use super::author::DimensionStyle;
use super::dash::DimDash;
use super::fit::FitCircle;
use super::group::{DimStandard, DimensionKind, DimensionRecord, Group};
use crate::vector::Point;
use crate::vector::linepick::{
    ParallelPolicy, PickedLine, TwoLineRelation, classify_two_lines, measured_angle_degrees,
};

/// How a centre mark is drawn, beyond its size (which is a style property,
/// [`DimensionStyle::centre_mark_size`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CentreMarkSpec {
    /// Draw extension lines from the cross out past the rim. Off by default:
    /// the bare cross is what both ANSI and ISO practice draw unless asked.
    pub extension_lines: bool,
    /// Direction of the first arm, degrees counter-clockwise from page +x;
    /// the second arm is 90° on. Carried so a rotated ce dimension's mark
    /// rotates with it.
    pub angle: f64,
}

/// The size of a centre mark: how far one cross arm reaches from the centre.
///
/// ANSI and ISO practice both state it either relative to the lettering
/// height or as a fixed length, so both are offered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CentreMarkSize {
    /// The arm is this multiple of the ce dimension's text height. Tracks a
    /// text-height restyle, which is why it is the default.
    TextHeight(f64),
    /// The arm is this many points, whatever the text height.
    Fixed(f64),
}

impl Default for CentreMarkSize {
    /// Half the text height: a 10 pt label gets a 10 pt cross.
    fn default() -> Self {
        Self::TextHeight(0.5)
    }
}

impl CentreMarkSize {
    /// The arm length in points for `text_height`.
    #[must_use]
    pub fn arm(self, text_height: f64) -> f64 {
        match self {
            Self::TextHeight(k) => k * text_height,
            Self::Fixed(pt) => pt,
        }
    }

    /// Whether the value is drawable: finite and positive.
    #[must_use]
    pub fn is_valid(self) -> bool {
        let v = match self {
            Self::TextHeight(v) | Self::Fixed(v) => v,
        };
        v.is_finite() && v > 0.0
    }

    /// Parse `text:<factor>` or `fixed:<points>`, the CLI and sidecar
    /// spelling. `None` for anything else or an undrawable value.
    ///
    /// ```
    /// use pdfcer_core::dimension::CentreMarkSize;
    ///
    /// assert_eq!(CentreMarkSize::parse("text:0.5"), Some(CentreMarkSize::TextHeight(0.5)));
    /// assert_eq!(CentreMarkSize::parse("fixed:9"), Some(CentreMarkSize::Fixed(9.0)));
    /// assert_eq!(CentreMarkSize::parse("fixed:-1"), None);
    /// ```
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let (kind, value) = s.split_once(':')?;
        let v: f64 = value.trim().parse().ok()?;
        let size = match kind.trim().to_ascii_lowercase().as_str() {
            "text" => Self::TextHeight(v),
            "fixed" => Self::Fixed(v),
            _ => return None,
        };
        size.is_valid().then_some(size)
    }

    /// The `text:<factor>` / `fixed:<points>` spelling [`Self::parse`] reads.
    #[must_use]
    pub fn token(self) -> String {
        match self {
            Self::TextHeight(v) => format!("text:{v}"),
            Self::Fixed(v) => format!("fixed:{v}"),
        }
    }
}

impl CentreMarkSpec {
    /// This mark with its arms turned `degrees` counter-clockwise.
    #[must_use]
    pub fn turned(self, degrees: f64) -> Self {
        Self {
            angle: (self.angle + degrees).rem_euclid(360.0),
            ..self
        }
    }
}

/// How far a centre line runs past the end of the overlap it spans, in points
/// (1/8 in), unless the caller asks for another overshoot.
pub const DEFAULT_CENTRE_LINE_OVERSHOOT: f64 = 9.0;

/// Why pdfcer declined to author a centre line from two picked lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CentreLineRefusal {
    /// The lines lie on one infinite line, so there is nothing between them.
    #[error("those two lines are collinear, so there is no centre line between them")]
    Collinear,
    /// One line has zero length, so it has no direction.
    #[error("one of those lines has zero length; two distinct points are needed per line")]
    Degenerate,
    /// Projected onto the centre line's direction, the two lines share no
    /// length, so no span is common to both.
    #[error(
        "those two lines do not overlap along the centre line, so there is no span to draw it \
         over"
    )]
    NoOverlap,
    /// The overshoot was negative or not a number.
    #[error("the centre-line overshoot must be a finite length of zero or more")]
    BadOvershoot,
}

/// A centre line authored from two picked lines, with the evidence behind it
/// (project rule 4: the shell discloses what was inferred).
#[derive(Debug, Clone, PartialEq)]
pub struct CentreLineAuthoring {
    /// The [`DimensionKind::CentreLine`] to hand to
    /// `EditSession::add_dimension`.
    pub kind: DimensionKind,
    /// What the two lines were read as: parallel (a midline) or angled (a
    /// bisector). Never collinear.
    pub relation: TwoLineRelation,
    /// The true angle between the lines, folded into `[0, 90]`, reported even
    /// when the parallel reading was forced.
    pub measured_angle_degrees: Option<f64>,
    /// Whether the parallel reading came from the operator's override.
    pub forced_parallel: bool,
    /// Length of the span common to both lines, before the overshoot.
    pub overlap: f64,
}

/// Author the centre line between two picked lines.
///
/// | relation | result |
/// |---|---|
/// | parallel (or forced) | the midline, over the overlap of the two lines |
/// | angled | the bisector of the wedge the picks select, over the overlap of both lines projected onto it |
/// | collinear | [`CentreLineRefusal::Collinear`] |
///
/// Either end then runs `overshoot` points further, as drafting practice
/// draws a centre line past the feature ([`DEFAULT_CENTRE_LINE_OVERSHOOT`]),
/// except that a bisector never overshoots back through its apex: past the
/// apex it would lie in the opposite angle, which it does not bisect.
/// The bisector is the one between the arms pointing from the apex toward
/// each pick, the same rule that chooses the angle of an angular ce
/// dimension.
///
/// # Errors
///
/// [`CentreLineRefusal`] for collinear or zero-length lines, lines that share
/// no span, or an invalid overshoot.
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::{DimensionKind, author_centre_line};
/// use pdfcer_core::vector::linepick::{ParallelPolicy, PickedLine};
/// use pdfcer_core::vector::{HitTarget, Point};
///
/// let mk = |sx, sy, ex, ey| PickedLine {
///     target: HitTarget::Object(0),
///     subpath: 0,
///     segment: 0,
///     start: Point::new(sx, sy),
///     end: Point::new(ex, ey),
///     pick: Point::new(f64::midpoint(sx, ex), f64::midpoint(sy, ey)),
/// };
/// let authored = author_centre_line(
///     &mk(100.0, 100.0, 300.0, 100.0),
///     &mk(150.0, 140.0, 350.0, 140.0),
///     ParallelPolicy::from_setting(0.5),
///     9.0,
/// )?;
/// let DimensionKind::CentreLine { a, b } = authored.kind else { unreachable!() };
/// assert_eq!((a.x, a.y, b.x, b.y), (141.0, 120.0, 309.0, 120.0));
/// # Ok::<(), pdfcer_core::dimension::CentreLineRefusal>(())
/// ```
pub fn author_centre_line(
    a: &PickedLine,
    b: &PickedLine,
    policy: ParallelPolicy,
    overshoot: f64,
) -> Result<CentreLineAuthoring, CentreLineRefusal> {
    if !(overshoot.is_finite() && overshoot >= 0.0) {
        return Err(CentreLineRefusal::BadOvershoot);
    }
    let relation = classify_two_lines(a, b, policy).ok_or(CentreLineRefusal::Degenerate)?;
    let (ux, uy) = a.direction().ok_or(CentreLineRefusal::Degenerate)?;
    let (origin, dir) = match relation {
        TwoLineRelation::Collinear => return Err(CentreLineRefusal::Collinear),
        TwoLineRelation::Parallel { .. } => {
            let n = Point::new(-uy, ux);
            let half = dot(sub(b.pick, a.start), n) / 2.0;
            (
                Point::new(n.x.mul_add(half, a.start.x), n.y.mul_add(half, a.start.y)),
                Point::new(ux, uy),
            )
        }
        TwoLineRelation::Angled { apex, .. } => (
            apex,
            bisector(apex, a, b).ok_or(CentreLineRefusal::Degenerate)?,
        ),
    };
    let span = |l: &PickedLine| {
        let (s, e) = (dot(sub(l.start, origin), dir), dot(sub(l.end, origin), dir));
        (s.min(e), s.max(e))
    };
    let ((a_lo, a_hi), (b_lo, b_hi)) = (span(a), span(b));
    let (lo, hi) = (a_lo.max(b_lo), a_hi.min(b_hi));
    if hi - lo <= a.length().max(b.length()).max(1.0) * 1e-6 {
        return Err(CentreLineRefusal::NoOverlap);
    }
    let at = |t: f64| Point::new(dir.x.mul_add(t, origin.x), dir.y.mul_add(t, origin.y));
    let floor = match relation {
        TwoLineRelation::Angled { .. } => lo.min(0.0),
        _ => f64::NEG_INFINITY,
    };
    Ok(CentreLineAuthoring {
        kind: DimensionKind::CentreLine {
            a: at((lo - overshoot).max(floor)),
            b: at(hi + overshoot),
        },
        relation,
        measured_angle_degrees: measured_angle_degrees(a, b),
        forced_parallel: policy.force_parallel,
        overlap: hi - lo,
    })
}

/// Unit bisector of the wedge from `apex` toward each line's pick; falls
/// back to each line's own direction when a pick sits on the apex.
fn bisector(apex: Point, a: &PickedLine, b: &PickedLine) -> Option<Point> {
    let arm = |l: &PickedLine| {
        let d = sub(l.pick, apex);
        let len = d.x.hypot(d.y);
        if len > f64::EPSILON {
            Some(Point::new(d.x / len, d.y / len))
        } else {
            l.direction().map(|(x, y)| Point::new(x, y))
        }
    };
    let (ua, ub) = (arm(a)?, arm(b)?);
    let (sx, sy) = (ua.x + ub.x, ua.y + ub.y);
    let len = sx.hypot(sy);
    (len > 1e-9).then(|| Point::new(sx / len, sy / len))
}

fn sub(p: Point, q: Point) -> Point {
    Point::new(p.x - q.x, p.y - q.y)
}

fn dot(p: Point, q: Point) -> f64 {
    p.x.mul_add(q.x, p.y * q.y)
}

/// The stroked segments of a centre mark on `fit`, arm length `arm`: the two
/// cross arms, then (with `extension_lines`, and only where the rim lies
/// beyond the gap) four extension lines.
#[must_use]
pub(crate) fn mark_segments(fit: FitCircle, spec: CentreMarkSpec, arm: f64) -> Vec<(Point, Point)> {
    let c = fit.center;
    let ray = |deg: f64, r: f64| {
        let (s, co) = deg.to_radians().sin_cos();
        Point::new(co.mul_add(r, c.x), s.mul_add(r, c.y))
    };
    let mut out = vec![
        (ray(spec.angle, arm), ray(spec.angle + 180.0, arm)),
        (ray(spec.angle + 90.0, arm), ray(spec.angle + 270.0, arm)),
    ];
    let (from, to) = (2.0 * arm, fit.radius + arm);
    if spec.extension_lines && to > from {
        for q in 0..4 {
            let deg = f64::from(q).mul_add(90.0, spec.angle);
            out.push((ray(deg, from), ray(deg, to)));
        }
    }
    out
}

impl DimensionStyle {
    /// The nominal long-short-long centre-line dash for this style's drafting
    /// standard, in points: ANSI after the `CENTER2` linetype
    /// (`[54 9 9 9]`, 3/4 in long), ISO-style after ISO 128's proportions to
    /// the line width (`[24w 3w w 3w]`; ISO's short dash of `0.5w` is drawn
    /// at `w` so it stays visible). The factory centre-line pattern
    /// [`Self::centre_line_stroke`] uses when `centre.line.dash` is unset; it
    /// never reads `dash`.
    #[must_use]
    pub fn centre_line_dash(self) -> DimDash {
        let w = self.line_width;
        let runs = match self.standard {
            DimStandard::Ansi => [54.0, 9.0, 9.0, 9.0],
            DimStandard::Iso => [24.0 * w, 3.0 * w, w, 3.0 * w],
        };
        DimDash::new(&runs).unwrap_or(DimDash::SOLID)
    }

    /// The centre-mark arm length in points.
    #[must_use]
    pub fn centre_mark_arm(self) -> f64 {
        self.centre_mark_size.arm(self.text_height)
    }
}

/// What a snap point on a centre line or centre mark is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CentreSnapRole {
    /// An end of a centre line.
    LineEnd,
    /// The midpoint of a centre line.
    LineMidpoint,
    /// A centre mark's centre.
    MarkCentre,
    /// The outer end of a centre-mark cross arm.
    ArmEnd,
    /// The outer end of a centre-mark extension line.
    ExtensionEnd,
}

/// One snap-relevant point on a centre line or centre mark, page space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CentreSnapPoint {
    /// Where it is, page space, points.
    pub point: Point,
    /// What it is.
    pub role: CentreSnapRole,
}

impl DimensionKind {
    /// The centre line's ends, or the centre mark's stroked segments (cross
    /// arms, then extension lines), for snapping to the nearest point along
    /// them. Empty for a kind with no centre geometry. `style` sizes the
    /// mark; resolve it with [`super::resolve_style`].
    #[must_use]
    pub fn centre_snap_segments(&self, style: DimensionStyle) -> Vec<(Point, Point)> {
        match *self {
            Self::CentreLine { a, b } => vec![(a, b)],
            Self::CentreMark { fit, mark } => mark_segments(fit, mark, style.centre_mark_arm()),
            Self::Circular {
                fit,
                centre_mark: Some(mark),
                ..
            } => mark_segments(fit, mark, style.centre_mark_arm()),
            _ => Vec::new(),
        }
    }

    /// The snap points of a centre line (ends, midpoint) or a centre mark
    /// (centre, arm ends, extension-line ends), including the mark a
    /// [`Self::Circular`] carries. Empty for any other kind.
    #[must_use]
    pub fn centre_snap_points(&self, style: DimensionStyle) -> Vec<CentreSnapPoint> {
        let at = |point, role| CentreSnapPoint { point, role };
        if let Self::CentreLine { a, b } = *self {
            return vec![
                at(a, CentreSnapRole::LineEnd),
                at(b, CentreSnapRole::LineEnd),
                at(a.midpoint(b), CentreSnapRole::LineMidpoint),
            ];
        }
        let Some(centre) = self.centre_mark_centre() else {
            return Vec::new();
        };
        let mut out = vec![at(centre, CentreSnapRole::MarkCentre)];
        for (i, (p, q)) in self.centre_snap_segments(style).into_iter().enumerate() {
            if i < 2 {
                out.push(at(p, CentreSnapRole::ArmEnd));
                out.push(at(q, CentreSnapRole::ArmEnd));
            } else {
                out.push(at(q, CentreSnapRole::ExtensionEnd));
            }
        }
        out
    }

    /// A centre line or centre mark carried through the affine point map
    /// `map`: points are mapped, the mark's arms turn with the map's image of
    /// the first arm, and the radius scales by the map's area factor (as
    /// [`super::transform_kind`] scales a circular radius). Any other kind is
    /// returned unchanged.
    #[must_use]
    pub(crate) fn centre_mapped(self, map: impl Fn(Point) -> Point) -> Self {
        match self {
            Self::CentreLine { a, b } => Self::CentreLine {
                a: map(a),
                b: map(b),
            },
            Self::CentreMark { fit, mark } => {
                let c = map(fit.center);
                let (s, co) = mark.angle.to_radians().sin_cos();
                let ex = sub(map(Point::new(fit.center.x + co, fit.center.y + s)), c);
                let ey = sub(map(Point::new(fit.center.x - s, fit.center.y + co)), c);
                let angle = if ex.x.hypot(ex.y) > 0.0 {
                    ex.y.atan2(ex.x).to_degrees().rem_euclid(360.0)
                } else {
                    mark.angle
                };
                let area = ex.x.mul_add(ey.y, -(ex.y * ey.x)).abs().sqrt();
                Self::CentreMark {
                    fit: FitCircle {
                        center: c,
                        radius: fit.radius * area,
                        ..fit
                    },
                    mark: CentreMarkSpec { angle, ..mark },
                }
            }
            other => other,
        }
    }

    /// The centre of the mark this kind draws, if it draws one.
    #[must_use]
    pub const fn centre_mark_centre(&self) -> Option<Point> {
        match *self {
            Self::CentreMark { fit, .. }
            | Self::Circular {
                fit,
                centre_mark: Some(_),
                ..
            } => Some(fit.center),
            _ => None,
        }
    }

    /// Whether this kind is centre geometry that measures nothing
    /// ([`Self::CentreLine`] or [`Self::CentreMark`]): no label, no
    /// `/Measure`, no text override and no placement.
    #[must_use]
    pub const fn is_centre_geometry(&self) -> bool {
        matches!(self, Self::CentreLine { .. } | Self::CentreMark { .. })
    }
}

impl DimensionRecord {
    /// [`DimensionKind::centre_snap_points`] with this record's resolved
    /// style in `group` (its own overrides honoured).
    #[must_use]
    pub fn centre_snap_points(&self, group: &Group) -> Vec<CentreSnapPoint> {
        self.kind
            .centre_snap_points(super::resolve_style(group, &self.style))
    }
}

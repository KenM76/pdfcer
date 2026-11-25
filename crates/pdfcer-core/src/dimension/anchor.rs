//! Associative anchors (pdfcer-gui request G332): a measured point of a ce
//! dimension bound to a point of another ce dimension's centre geometry, so
//! the ce dimension follows it when it moves.
//!
//! Only style-independent points anchor: a centre, a centre-line end,
//! midpoint or axis, and a circle's rim. Arm and extension-line ends move
//! with the style, so a restyle never re-solves anything.
//!
//! Anchorable points ([`DimensionKind::anchor_point_count`]): a linear ce
//! dimension's `a` (0) and `b` (1), an angular one's apex (0), a perimeter's
//! vertices. Targets: a centre line, a centre mark, a radius/diameter ce
//! dimension. The two sets do not overlap, so a chain of anchors is one link
//! long and no cycle can form; [`would_cycle`] guards the general case.
//!
//! The solved points stay on the kind as a cache: a build that drops the
//! anchor keys still draws and measures the ce dimension as last solved.

use std::fmt;

use super::group::{DimensionEnd, DimensionId, DimensionKind, DimensionModel, DimensionRecord};
use crate::object::{Dict, Name, Object};
use crate::vector::{AxisConstraint, Point};

/// Which rim point a [`AnchorRole::Rim`] anchor takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ArcCondition {
    /// The circle's centre.
    Centre,
    /// The rim point nearest the ce dimension's other end.
    Min,
    /// The rim point farthest from the ce dimension's other end.
    Max,
}

/// Which point of the target an anchor takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AnchorRole {
    /// A centre mark's centre, or a radius/diameter ce dimension's centre
    /// (with or without a mark).
    MarkCentre,
    /// An end of a centre line.
    LineEnd(DimensionEnd),
    /// A centre line's midpoint.
    LineMidpoint,
    /// A centre line taken as an infinite line: the foot of the
    /// perpendicular from the other end (a linear ce dimension) or from the
    /// point itself (any other kind). Under a horizontal or vertical
    /// constraint, the point on the line level with the other end.
    LineAxis,
    /// The rim of a radius/diameter ce dimension's circle or of the circle a
    /// centre mark was fitted to.
    Rim(ArcCondition),
}

impl AnchorRole {
    /// Every role, in a fixed order.
    pub const ALL: [Self; 8] = [
        Self::MarkCentre,
        Self::LineEnd(DimensionEnd::A),
        Self::LineEnd(DimensionEnd::B),
        Self::LineMidpoint,
        Self::LineAxis,
        Self::Rim(ArcCondition::Centre),
        Self::Rim(ArcCondition::Min),
        Self::Rim(ArcCondition::Max),
    ];

    /// The stable token: `mark-centre`, `line-end-a`, `line-end-b`,
    /// `line-midpoint`, `line-axis`, `rim-centre`, `rim-min`, `rim-max`.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::MarkCentre => "mark-centre",
            Self::LineEnd(DimensionEnd::A) => "line-end-a",
            Self::LineEnd(DimensionEnd::B) => "line-end-b",
            Self::LineMidpoint => "line-midpoint",
            Self::LineAxis => "line-axis",
            Self::Rim(ArcCondition::Centre) => "rim-centre",
            Self::Rim(ArcCondition::Min) => "rim-min",
            Self::Rim(ArcCondition::Max) => "rim-max",
        }
    }

    /// The role whose [`Self::token`] is `text`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.token() == text)
    }

    /// Whether this role needs the ce dimension's other end (`Rim(Min)` and
    /// `Rim(Max)`), which only a linear ce dimension has.
    #[must_use]
    pub const fn needs_other_end(self) -> bool {
        matches!(self, Self::Rim(ArcCondition::Min | ArcCondition::Max))
    }

    /// Whether `target` has the point this role takes.
    #[must_use]
    pub const fn fits(self, target: &DimensionKind) -> bool {
        match self {
            Self::MarkCentre | Self::Rim(_) => matches!(
                target,
                DimensionKind::Circular { .. } | DimensionKind::CentreMark { .. }
            ),
            Self::LineEnd(_) | Self::LineMidpoint | Self::LineAxis => {
                matches!(target, DimensionKind::CentreLine { .. })
            }
        }
    }
}

/// One measured point's binding to a target ce dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Anchor {
    /// The ce dimension whose geometry the point follows.
    pub target: DimensionId,
    /// Which of its points.
    pub role: AnchorRole,
}

impl Anchor {
    /// An anchor to `role` of `target`.
    ///
    /// ```
    /// use pdfcer_core::dimension::{Anchor, AnchorRole, ArcCondition, DimensionId};
    /// let a = Anchor::new(DimensionId(2), AnchorRole::Rim(ArcCondition::Max));
    /// assert_eq!(a.role.token(), "rim-max");
    /// ```
    #[must_use]
    pub const fn new(target: DimensionId, role: AnchorRole) -> Self {
        Self { target, role }
    }
}

/// Why an anchor could not be solved. The point keeps its last position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unsolvable {
    /// The target is not in the document.
    TargetMissing,
    /// The target has no point of this role (its kind changed).
    RoleNotOnTarget,
    /// `Rim(Min)` / `Rim(Max)` with the other end on the circle's centre
    /// (or level with it, under a constraint): no direction is defined.
    OtherEndOnCentre,
    /// `LineAxis` under a constraint parallel to the line: no point on the
    /// line is level with the other end.
    AxisParallel,
    /// The centre line has zero length.
    DegenerateLine,
    /// The anchored point index is past the ce dimension's points.
    PointMissing,
}

impl fmt::Display for Unsolvable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TargetMissing => "the target ce dimension is not in the document",
            Self::RoleNotOnTarget => "the target has no such point",
            Self::OtherEndOnCentre => "the other end is on the circle's centre",
            Self::AxisParallel => "the centre line is parallel to the constraint",
            Self::DegenerateLine => "the centre line has zero length",
            Self::PointMissing => "the ce dimension has no such point",
        })
    }
}

/// Whether an anchor solved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AnchorState {
    /// The point sits where the anchor puts it.
    Solved,
    /// The point kept its last position, for this reason.
    Unsolvable(Unsolvable),
}

/// One ce dimension re-solved because a target it is anchored to changed,
/// or detached because its target was deleted. Shells list these
/// off-canvas: the value changed on a ce dimension the operator did not
/// touch.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Resolved {
    /// The dependent ce dimension.
    pub id: DimensionId,
    /// Its label before (empty for centre geometry).
    pub before_label: String,
    /// Its label after.
    pub after_label: String,
    /// `true` when the anchor was cleared (its target was deleted) rather
    /// than followed; the points stayed where they were.
    pub detached: bool,
}

impl Resolved {
    /// A ce dimension that followed its target.
    #[must_use]
    pub const fn new(id: DimensionId, before_label: String, after_label: String) -> Self {
        Self {
            id,
            before_label,
            after_label,
            detached: false,
        }
    }

    /// A ce dimension whose anchor was cleared; its label is unchanged.
    #[must_use]
    pub fn detached(id: DimensionId, label: String) -> Self {
        Self {
            id,
            before_label: label.clone(),
            after_label: label,
            detached: true,
        }
    }
}

/// What [`crate::edit::EditSession::set_dimension_anchor`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AnchorOutcome {
    /// The ce dimension's label before.
    pub before_label: String,
    /// Its label after the anchor was solved.
    pub after_label: String,
    /// Whether the new anchor solved; `None` when it was cleared.
    pub state: Option<AnchorState>,
}

impl AnchorOutcome {
    /// An outcome.
    #[must_use]
    pub const fn new(
        before_label: String,
        after_label: String,
        state: Option<AnchorState>,
    ) -> Self {
        Self {
            before_label,
            after_label,
            state,
        }
    }
}

impl DimensionKind {
    /// How many measured points can carry an anchor: 2 for a linear ce
    /// dimension, 1 (the apex) for an angular one, every vertex of a
    /// perimeter, 0 for the rest.
    #[must_use]
    pub fn anchor_point_count(&self) -> usize {
        match self {
            Self::Linear { .. } => 2,
            Self::Angular { .. } => 1,
            Self::Perimeter { points, .. } => points.len(),
            _ => 0,
        }
    }

    /// Measured point `index` (see [`Self::anchor_point_count`]).
    #[must_use]
    pub fn anchor_point(&self, index: usize) -> Option<Point> {
        match (self, index) {
            (Self::Linear { a, .. }, 0) => Some(*a),
            (Self::Linear { b, .. }, 1) => Some(*b),
            (Self::Angular { apex, .. }, 0) => Some(*apex),
            (Self::Perimeter { points, .. }, i) => points.get(i).copied(),
            _ => None,
        }
    }

    fn set_anchor_point(&mut self, index: usize, to: Point) {
        match (self, index) {
            (Self::Linear { a, .. }, 0) => *a = to,
            (Self::Linear { b, .. }, 1) => *b = to,
            (Self::Angular { apex, .. }, 0) => *apex = to,
            (Self::Perimeter { points, .. }, i) => {
                if let Some(p) = points.get_mut(i) {
                    *p = to;
                }
            }
            _ => {}
        }
    }

    const fn constraint(&self) -> AxisConstraint {
        match self {
            Self::Linear { constraint, .. } => *constraint,
            _ => AxisConstraint::Aligned,
        }
    }
}

const EPS: f64 = 1e-9;

fn circle_of(target: &DimensionKind) -> Option<(Point, f64)> {
    match target {
        DimensionKind::Circular { fit, .. } | DimensionKind::CentreMark { fit, .. } => {
            Some((fit.center, fit.radius))
        }
        _ => None,
    }
}

/// The rim point of circle `(c, r)` nearest (`min`) or farthest from
/// `reference`, measured along the constraint's axis.
fn rim_point(
    (c, r): (Point, f64),
    reference: Point,
    min: bool,
    constraint: AxisConstraint,
) -> Result<Point, Unsolvable> {
    let toward = |v: f64| if min { v } else { -v };
    match constraint {
        AxisConstraint::Horizontal => {
            let dx = reference.x - c.x;
            if dx.abs() < EPS {
                return Err(Unsolvable::OtherEndOnCentre);
            }
            Ok(Point::new(c.x + toward(r * dx.signum()), c.y))
        }
        AxisConstraint::Vertical => {
            let dy = reference.y - c.y;
            if dy.abs() < EPS {
                return Err(Unsolvable::OtherEndOnCentre);
            }
            Ok(Point::new(c.x, c.y + toward(r * dy.signum())))
        }
        AxisConstraint::Aligned => {
            let (dx, dy) = (reference.x - c.x, reference.y - c.y);
            let len = dx.hypot(dy);
            if len < EPS {
                return Err(Unsolvable::OtherEndOnCentre);
            }
            Ok(Point::new(
                c.x + toward(r * dx / len),
                c.y + toward(r * dy / len),
            ))
        }
    }
}

/// The point on the infinite line `a`–`b` that `reference` projects to
/// along the constraint.
fn axis_point(
    a: Point,
    b: Point,
    reference: Point,
    constraint: AxisConstraint,
) -> Result<Point, Unsolvable> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx.mul_add(dx, dy * dy);
    if len2 < EPS * EPS {
        return Err(Unsolvable::DegenerateLine);
    }
    let t = match constraint {
        AxisConstraint::Horizontal if dy.abs() < EPS => return Err(Unsolvable::AxisParallel),
        AxisConstraint::Horizontal => (reference.y - a.y) / dy,
        AxisConstraint::Vertical if dx.abs() < EPS => return Err(Unsolvable::AxisParallel),
        AxisConstraint::Vertical => (reference.x - a.x) / dx,
        AxisConstraint::Aligned => (reference.x - a.x).mul_add(dx, (reference.y - a.y) * dy) / len2,
    };
    Ok(Point::new(t.mul_add(dx, a.x), t.mul_add(dy, a.y)))
}

/// Where `role` of `target` puts a point, given the reference the role
/// measures from (the other end, or the point itself).
fn target_point(
    target: &DimensionKind,
    role: AnchorRole,
    reference: Point,
    constraint: AxisConstraint,
) -> Result<Point, Unsolvable> {
    if !role.fits(target) {
        return Err(Unsolvable::RoleNotOnTarget);
    }
    match (role, target) {
        (AnchorRole::LineEnd(DimensionEnd::A), DimensionKind::CentreLine { a, .. }) => Ok(*a),
        (AnchorRole::LineEnd(DimensionEnd::B), DimensionKind::CentreLine { b, .. }) => Ok(*b),
        (AnchorRole::LineMidpoint, DimensionKind::CentreLine { a, b }) => Ok(a.midpoint(*b)),
        (AnchorRole::LineAxis, DimensionKind::CentreLine { a, b }) => {
            axis_point(*a, *b, reference, constraint)
        }
        (AnchorRole::Rim(ArcCondition::Min | ArcCondition::Max), _) => {
            let circle = circle_of(target).ok_or(Unsolvable::RoleNotOnTarget)?;
            let min = role == AnchorRole::Rim(ArcCondition::Min);
            rim_point(circle, reference, min, constraint)
        }
        _ => circle_of(target)
            .map(|(c, _)| c)
            .ok_or(Unsolvable::RoleNotOnTarget),
    }
}

/// The point the reference for `point`'s anchor is taken from: the other end
/// of a linear ce dimension (its target's centre when that end is itself on
/// a rim, which is exact for nearest and farthest between two circles), or
/// the point itself.
fn reference_for(
    model: &DimensionModel,
    record: &DimensionRecord,
    kind: &DimensionKind,
    point: usize,
) -> Option<Point> {
    if !matches!(kind, DimensionKind::Linear { .. }) {
        return kind.anchor_point(point);
    }
    let other = 1 - point.min(1);
    let other_rim = record
        .anchors
        .iter()
        .find(|(p, a)| *p == other && a.role.needs_other_end());
    if let Some((_, anchor)) = other_rim
        && let Some((c, _)) = model
            .dimension(anchor.target)
            .and_then(|t| circle_of(&t.kind))
    {
        return Some(c);
    }
    kind.anchor_point(other)
}

/// Solve one anchor of `record` against `model` on top of `kind`.
fn solve_one(
    model: &DimensionModel,
    record: &DimensionRecord,
    kind: &DimensionKind,
    (point, anchor): (usize, Anchor),
) -> Result<Point, Unsolvable> {
    if point >= kind.anchor_point_count() {
        return Err(Unsolvable::PointMissing);
    }
    let target = model
        .dimension(anchor.target)
        .filter(|t| t.id != record.id)
        .ok_or(Unsolvable::TargetMissing)?;
    let reference = reference_for(model, record, kind, point).ok_or(Unsolvable::PointMissing)?;
    target_point(&target.kind, anchor.role, reference, kind.constraint())
}

/// `record`'s kind with every anchor solved, and each anchor's state in
/// `record.anchors` order. Anchors that need the other end are solved after
/// the rest, so they measure from where the other end now is. An unsolvable
/// anchor leaves its point where it was.
#[must_use]
pub fn solve_anchors(
    model: &DimensionModel,
    record: &DimensionRecord,
) -> (DimensionKind, Vec<AnchorState>) {
    let mut kind = record.kind.clone();
    let mut states = vec![AnchorState::Solved; record.anchors.len()];
    for pass_needs_other in [false, true] {
        for (&binding, state) in record.anchors.iter().zip(states.iter_mut()) {
            if binding.1.role.needs_other_end() != pass_needs_other {
                continue;
            }
            match solve_one(model, record, &kind, binding) {
                Ok(p) if p.is_finite() => kind.set_anchor_point(binding.0, p),
                Ok(_) => *state = AnchorState::Unsolvable(Unsolvable::OtherEndOnCentre),
                Err(why) => *state = AnchorState::Unsolvable(why),
            }
        }
    }
    (kind, states)
}

/// Whether binding `dependent` to `target` would close a loop: `target` is
/// `dependent`, or follows it through anchors. Bounded by the model size.
#[must_use]
pub fn would_cycle(model: &DimensionModel, dependent: DimensionId, target: DimensionId) -> bool {
    let mut frontier = vec![target];
    let mut seen = Vec::new();
    while let Some(at) = frontier.pop() {
        if at == dependent {
            return true;
        }
        if seen.contains(&at) || seen.len() > model.dimensions().len() {
            continue;
        }
        seen.push(at);
        if let Some(r) = model.dimension(at) {
            frontier.extend(r.anchors.iter().map(|(_, a)| a.target));
        }
    }
    false
}

impl DimensionModel {
    /// Every ce dimension with a point anchored to `target`, in model order.
    #[must_use]
    pub fn anchor_dependents(&self, target: DimensionId) -> Vec<DimensionId> {
        self.dimensions()
            .iter()
            .filter(|r| r.anchors.iter().any(|(_, a)| a.target == target))
            .map(|r| r.id)
            .collect()
    }

    /// Clear every anchor to `target`, keeping the points where they are;
    /// the ce dimensions detached.
    pub fn detach_anchors_to(&mut self, target: DimensionId) -> Vec<DimensionId> {
        let ids = self.anchor_dependents(target);
        for id in &ids {
            if let Some(r) = self.dimension_mut(*id) {
                r.anchors.retain(|(_, a)| a.target != target);
            }
        }
        ids
    }

    /// Clear every anchor whose target is not in this model (a ce dimension
    /// carried into another document without its target); how many were
    /// cleared.
    pub fn detach_missing_targets(&mut self) -> usize {
        let present: Vec<DimensionId> = self.dimensions().iter().map(|r| r.id).collect();
        let mut cleared = 0;
        for id in present.clone() {
            if let Some(r) = self.dimension_mut(id) {
                let before = r.anchors.len();
                r.anchors.retain(|(_, a)| present.contains(&a.target));
                cleared += before - r.anchors.len();
            }
        }
        cleared
    }
}

/// Write `anchors` as `/Anchors [<< /Point i /Target t /Role /name >> ...]`
/// (sidecar version 11); nothing for none.
pub(crate) fn put_anchor_keys(d: &mut Dict, anchors: &[(usize, Anchor)]) {
    if anchors.is_empty() {
        return;
    }
    let entries = anchors
        .iter()
        .map(|(point, a)| {
            let mut e = Dict::new();
            e.insert(
                Name::from(b"Point"),
                Object::Integer(i64::try_from(*point).unwrap_or(i64::MAX)),
            );
            e.insert(
                Name::from(b"Target"),
                Object::Integer(i64::from(a.target.0)),
            );
            e.insert(
                Name::from(b"Role"),
                Object::Name(Name::from(a.role.token().as_bytes())),
            );
            Object::Dict(e)
        })
        .collect();
    d.insert(Name::from(b"Anchors"), Object::Array(entries));
}

/// Read `/Anchors`; an entry with an unknown role or a bad number is
/// dropped, so its point stays where it was last solved.
pub(crate) fn read_anchor_keys(d: &Dict) -> Vec<(usize, Anchor)> {
    let Some(Object::Array(entries)) = d.get(b"Anchors") else {
        return Vec::new();
    };
    let mut out: Vec<(usize, Anchor)> = entries
        .iter()
        .filter_map(|e| {
            let e = e.as_dict()?;
            let point = usize::try_from(e.get(b"Point")?.as_int()?).ok()?;
            let target = u32::try_from(e.get(b"Target")?.as_int()?).ok()?;
            let role = std::str::from_utf8(e.get(b"Role")?.as_name()?.as_bytes()).ok()?;
            Some((
                point,
                Anchor::new(DimensionId(target), AnchorRole::parse(role)?),
            ))
        })
        .collect();
    out.sort_by_key(|(p, _)| *p);
    out.dedup_by_key(|(p, _)| *p);
    out
}

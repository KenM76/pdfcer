//! Picking a straight LINE off the page, and deciding what two of them mean.
//!
//! # Why this module exists
//!
//! Every ce-dimension pick model in pdfcer up to now has been *point*-based:
//! [`crate::vector::snap`] answers "what point is near the cursor" and the
//! linear measure tool asks that question three times. That is the right
//! model for dimensioning between two arbitrary locations, and the wrong one
//! for the workflow a CAD operator expects, which is:
//!
//! > select two lines — if they are parallel, dimension the distance between
//! > them; if they meet at an angle, dimension the angle.
//!
//! To do that at all, something has to be able to answer "which LINE did the
//! operator click", as a pair of endpoints, rather than "which point". Before
//! this module nothing could: `snap_candidates` returns single points,
//! [`crate::vector::hit::hit_test_subpaths`] returns subpath *indices*, and
//! [`crate::vector::centerline`] returns two endpoints but only for a filled
//! thin quad (a line drawn as a bar), never an ordinary stroked line.
//!
//! # The pick point is part of the answer, not just a way of finding it
//!
//! [`PickedLine`] records **where on the line the operator clicked**, and that
//! is load-bearing rather than diagnostic. SolidWorks' own API documentation
//! states it plainly for `AddDimension2`: *"creating an angular dimension
//! between two lines gets different results based on which line endpoints are
//! selected"*, and warns that selecting an entity by name instead of by
//! coordinate *"causes unpredictable results … because you cannot be sure
//! which line endpoint is selected."*
//!
//! Two crossing lines bound **four** angles. Which one the operator wants is
//! decided by which side of each line they clicked on — so a pick model that
//! stored only "line A, line B" would be unable to reproduce the behaviour
//! being copied, and would have to guess. Sourced to
//! `D:\Dev\Rag-Specialized\SolidWorks_Dimensions\solidworks__dimension_and_tolerance_options.md`
//! §G.
//!
//! # What is deliberately NOT decided here
//!
//! How near parallel is parallel. See [`ParallelPolicy`].

use super::decompose::{PageObjects, PathObject, Segment, VectorObject};
use super::geometry::Point;
use super::hit::{HitTarget, hit_test_subpaths_of};

/// A straight segment the operator picked, with the point they picked it at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickedLine {
    /// **Which list, and which entry in it**, the picked line came from.
    ///
    /// # ★★★ THIS WAS A BARE `object_index: usize` AND THAT WAS THE BUG
    ///
    /// A `usize` can only name an entry in [`PageObjects::objects`], so the
    /// type itself made it impossible to answer a question about a line drawn
    /// inside a **form XObject** — those live in [`PageObjects::leaves`], a
    /// separate list, and there is no integer that distinguishes them.
    ///
    /// The consequence was not a degraded pick; it was **no pick at all**.
    /// Measured on the operator's own CAD drawing: 129,758 page objects, one
    /// form, and **10,256 leaves** — every one of them a candidate line, and
    /// every one invisible. The two-line dimension tool was *inert* on that
    /// document, and on any document whose drawing is wrapped in a form,
    /// which is what SolidWorks and most CAD exporters produce.
    ///
    /// ⇒ **A field whose type cannot express the answer does not fail loudly
    /// — it returns a confident wrong one, or nothing.** Changing it to
    /// [`HitTarget`] is a breaking change on purpose: every caller now has to
    /// state what it does about a line inside a form, and the ones that
    /// cannot handle it get a compile error rather than a silent miss.
    ///
    /// Use [`Self::page_object_index`] where the old `usize` was wanted.
    pub target: HitTarget,
    /// Which subpath within that path object.
    pub subpath: usize,
    /// Which segment within that subpath.
    pub segment: usize,
    /// The segment's first endpoint, in page space.
    pub start: Point,
    /// The segment's second endpoint, in page space.
    pub end: Point,
    /// **Where the operator actually clicked**, projected onto the segment.
    ///
    /// Not a diagnostic. For two lines that meet at an angle this is what
    /// selects which of the four angles is meant — see the module docs.
    pub pick: Point,
}

impl PickedLine {
    /// The index into [`PageObjects::objects`], or `None` if this line came
    /// from **inside a form XObject**.
    ///
    /// The migration path from the old `object_index` field, and deliberately
    /// an `Option` rather than a `usize` with a sentinel. A caller that
    /// unwraps it is stating that it does not handle form contents, in one
    /// visible place, instead of indexing the wrong list with a plausible
    /// number.
    #[must_use]
    pub const fn page_object_index(&self) -> Option<usize> {
        match self.target {
            HitTarget::Object(i) => Some(i),
            HitTarget::Leaf(_) => None,
        }
    }

    /// The segment's direction, normalised. `None` for a degenerate segment.
    ///
    /// Returns `None` rather than a zero vector so a caller cannot silently
    /// compute an angle against nothing; every consumer here is forced to say
    /// what it does about a zero-length line.
    #[must_use]
    pub fn direction(&self) -> Option<(f64, f64)> {
        let (dx, dy) = (self.end.x - self.start.x, self.end.y - self.start.y);
        let len = dx.hypot(dy);
        if len <= f64::EPSILON {
            return None;
        }
        Some((dx / len, dy / len))
    }

    /// The segment's length in page units.
    #[must_use]
    pub fn length(&self) -> f64 {
        (self.end.x - self.start.x).hypot(self.end.y - self.start.y)
    }
}

/// How close to parallel counts as parallel.
///
/// # Why this is a policy and not a constant
///
/// The threshold is **not defined by anything**. SolidWorks' documented
/// behaviour is that two angled lines produce an angular dimension, and the
/// corpus search for an epsilon, a snap rule or a near-parallel threshold
/// found nothing — the catalog records it as unverified and recommends asking
/// an experienced operator, because a long-time user knows from feel whether
/// a 0.05° pair should give an angle or a distance.
///
/// pdfcer's standing rule for exactly this situation (R169) is that a choice
/// no standard makes is a SETTING, not a hard-coded number. So the value is
/// carried in, defaulted somewhere an operator can change, and never buried
/// in the geometry code.
///
/// The default of half a degree is a judgement, and is documented as one:
/// CAD-exported geometry is usually exact, so a pair that is a hair off
/// parallel is far more likely to be a rounding artefact of the exporter than
/// a deliberate 0.3° taper. An operator dimensioning a genuine shallow taper
/// can lower it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParallelPolicy {
    /// Below this angle between the two lines, they count as parallel.
    ///
    /// Carried from `Settings::parallel_epsilon_degrees`, never read from a
    /// literal at a call site.
    pub epsilon_degrees: f64,
    /// **The operator said these two lines are parallel — believe them.**
    ///
    /// When `true`, [`classify_two_lines`] does not consult
    /// [`Self::epsilon_degrees`] at all and reports a parallel relation
    /// whatever the measured angle is.
    ///
    /// # Why an explicit override exists at all
    ///
    /// Requested by the operator directly (2026-08-12): *"When making or
    /// editing a dimension of this type, there should be a checkbox option to
    /// treat the two lines as parallel."*
    ///
    /// It matters because the automatic reading is a GUESS, and a global
    /// threshold cannot be right for every pair in every drawing. Two edges
    /// that are nominally parallel can arrive 0.8° apart from an exporter's
    /// rounding, or from a scan, or because the original drawing was slightly
    /// off — and the operator, looking at the part, knows which. Without an
    /// override the only remedy would be to change a global setting to author
    /// one dimension and change it back, which is how a setting becomes a
    /// thing people fight.
    ///
    /// This is the "fuzzy, never sneaky" side of project rule 4 working in
    /// the operator's favour: pdfcer's inference is visible and rejectable,
    /// and rejecting it does not mean undoing anything else.
    ///
    /// # What it does NOT do
    ///
    /// It does not move the lines and it does not fake the measurement. The
    /// distance reported is still the real perpendicular distance measured at
    /// the pick point; for lines that genuinely diverge, that distance is
    /// simply the one at the place the operator pointed. A caller that wants
    /// to disclose "these are 0.8° from parallel" still can, because the
    /// measured angle is available from [`measured_angle_degrees`].
    pub force_parallel: bool,
}

impl Default for ParallelPolicy {
    fn default() -> Self {
        Self {
            epsilon_degrees: 0.5,
            force_parallel: false,
        }
    }
}

impl ParallelPolicy {
    /// Build a policy from the operator's stored setting.
    ///
    /// The one conversion point between `Settings` and the geometry, so a
    /// caller cannot accidentally pass a literal where the operator's own
    /// value belongs.
    #[must_use]
    pub const fn from_setting(epsilon_degrees: f64) -> Self {
        Self {
            epsilon_degrees,
            force_parallel: false,
        }
    }

    /// The same policy with the operator's "treat as parallel" box ticked.
    #[must_use]
    pub const fn forcing_parallel(self) -> Self {
        Self {
            force_parallel: true,
            ..self
        }
    }
}

/// The raw angle between two lines, in degrees, folded into `[0, 90]`.
///
/// Exposed separately from [`classify_two_lines`] so a shell can DISCLOSE how
/// far from parallel a pair actually is — both when offering the "treat as
/// parallel" checkbox and after the operator has ticked it. A checkbox that
/// hides the number it is overriding would be asking for a decision while
/// withholding the fact it turns on.
///
/// Returns `None` if either line is degenerate.
#[must_use]
pub fn measured_angle_degrees(a: &PickedLine, b: &PickedLine) -> Option<f64> {
    let (ax, ay) = a.direction()?;
    let (bx, by) = b.direction()?;
    let cross = ax.mul_add(by, -(ay * bx)).abs();
    let dot = ax.mul_add(bx, ay * by).abs();
    Some(cross.atan2(dot).to_degrees())
}

/// What two picked lines mean geometrically.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TwoLineRelation {
    /// Parallel (within [`ParallelPolicy`]) and apart — a LINEAR ce dimension
    /// of the perpendicular distance between them.
    Parallel {
        /// Perpendicular distance between the two lines, page units.
        distance: f64,
    },
    /// Parallel and lying on the same infinite line, so the perpendicular
    /// distance is ~0.
    ///
    /// Reported distinctly rather than as `Parallel { distance: 0.0 }`
    /// because a zero-length dimension is not a useful drawing and the shell
    /// should say why rather than author one. SolidWorks' own behaviour here
    /// is unverified, so pdfcer declines rather than inventing one.
    Collinear,
    /// At an angle — an ANGULAR ce dimension.
    Angled {
        /// The angle at the apex, in degrees, in `(0, 180)`.
        ///
        /// This is the angle *selected by where the operator clicked*, not
        /// merely the smallest of the four. See the module docs.
        degrees: f64,
        /// Where the two infinite lines cross.
        ///
        /// May lie outside both segments — two lines that would only meet if
        /// extended still define an angle, and CAD drawings dimension exactly
        /// that case routinely.
        apex: Point,
        /// Whether `apex` lies within both picked segments. `false` means the
        /// apex is virtual and the ce dimension will need extension lines
        /// reaching out to it, which a shell should disclose.
        apex_is_real: bool,
    },
}

/// Pick the straight line nearest `point`.
///
/// Returns `None` when nothing straight is within `tolerance` — including the
/// case where the nearest thing is a curve. A Bézier is deliberately NOT
/// approximated by its chord: dimensioning "the line" of a curve would
/// measure something the drawing does not contain, and silently treating a
/// curve as a line is the kind of inference project rule 4 exists to stop.
///
/// # Which segment wins
///
/// The nearest one by perpendicular distance to the click. Subpath candidates
/// come from [`hit_test_subpaths`], which is already nearest-first, and every
/// straight segment inside each candidate subpath is then measured
/// individually — a subpath is often a polyline, and "the line I clicked" is
/// one of its segments, not all of them.
#[must_use]
pub fn pick_line(
    model: &PageObjects,
    object_index: usize,
    point: Point,
    tolerance: f64,
) -> Option<PickedLine> {
    let Some(VectorObject::Path(path)) = model.objects.get(object_index) else {
        return None;
    };
    pick_line_of(path, HitTarget::Object(object_index), point, tolerance)
}

/// [`pick_line`] against a path this caller already has in hand, told which
/// list it came from.
///
/// # ★ Why the caller supplies `target` rather than this deriving it
///
/// Because it cannot be derived. A [`PathObject`] carries no record of which
/// list holds it — a page object and a form leaf are the *same type*, and
/// that is deliberate: it is what lets one geometry implementation serve
/// both. The provenance is the caller's knowledge, so the caller states it,
/// and a caller that passes the wrong one is making a claim it can be held
/// to rather than tripping over an inference.
///
/// This is the same split [`hit_test_subpaths_of`] makes, for the same
/// reason: the geometry never needed the index; only the lookup did.
#[must_use]
pub fn pick_line_of(
    path: &PathObject,
    target: HitTarget,
    point: Point,
    tolerance: f64,
) -> Option<PickedLine> {
    let subpaths = path.page_subpaths();
    let mut best: Option<(f64, PickedLine)> = None;

    for sp_index in hit_test_subpaths_of(path, point, tolerance) {
        let Some(sp) = subpaths.get(sp_index) else {
            continue;
        };
        let mut from = sp.start;
        for (seg_index, seg) in sp.segments.iter().enumerate() {
            let to = seg.end();
            // Only straight segments. A Cubic is skipped, not chorded.
            if matches!(seg, Segment::Line { .. }) {
                let (dist, proj) = distance_to_segment(point, from, to);
                if dist <= tolerance && best.as_ref().is_none_or(|(d, _)| dist < *d) {
                    best = Some((
                        dist,
                        PickedLine {
                            target,
                            subpath: sp_index,
                            segment: seg_index,
                            start: from,
                            end: to,
                            pick: proj,
                        },
                    ));
                }
            }
            from = to;
        }
        // A closed subpath has an implicit closing segment back to the start
        // which carries no `Segment` of its own. It is a real edge on the
        // page and an operator can click it, so it is pickable — indexed one
        // past the last explicit segment, which is how the rest of this crate
        // already addresses it.
        if sp.closed && from != sp.start {
            let (dist, proj) = distance_to_segment(point, from, sp.start);
            if dist <= tolerance && best.as_ref().is_none_or(|(d, _)| dist < *d) {
                best = Some((
                    dist,
                    PickedLine {
                        target,
                        subpath: sp_index,
                        segment: sp.segments.len(),
                        start: from,
                        end: sp.start,
                        pick: proj,
                    },
                ));
            }
        }
    }
    best.map(|(_, line)| line)
}

/// Pick the straight line nearest `point` **anywhere on the page**.
///
/// [`pick_line`] answers "which segment of *this object* did the operator
/// click"; a canvas click does not come with an object index, so this is the
/// form a shell actually needs. Every object is offered to [`pick_line`] and
/// the nearest straight segment across all of them wins.
///
/// # Why the search is exhaustive rather than short-circuiting
///
/// Returning the first object with a hit would make the answer depend on
/// content-stream order — two edges a fraction of a point apart would resolve
/// to whichever was drawn first, which from the operator's side is
/// indistinguishable from the pick being random. Comparing every candidate
/// costs a pass over the page's objects once per click, which is nothing
/// beside a click, and makes "the nearest line wins" true rather than usually
/// true.
///
/// Ties are broken toward the LOWER object index, so a repeated click on the
/// same spot resolves the same way every time. An arbitrary rule, but a
/// stable one — the alternative is a pick that changes its mind.
///
/// Returns `None` when nothing straight is within `tolerance`, including when
/// the nearest thing is a curve ([`pick_line`] skips those deliberately).
///
/// # ★★★ FORM CONTENTS ARE SEARCHED, AND THEY DID NOT USED TO BE
///
/// Both [`PageObjects::objects`] and [`PageObjects::leaves`] are offered, so
/// a line drawn inside a form XObject is pickable. The result's
/// [`PickedLine::target`] says which list it came from.
///
/// Before `Pass 138.0` only the page's own list was searched, and a form is
/// not a line, so **a page whose drawing lives inside a form had nothing
/// pickable at all.** The tool was not degraded there; it was inert. That is
/// what most CAD exports look like — measured on the operator's own files:
///
/// | file | page objects | forms | leaves |
/// |---|---:|---:|---:|
/// | a composite conformance page | 28 | 4 | **242** |
/// | a CAD drawing | 129,758 | 1 | **10,256** |
/// | a flat export | 5,903 | 0 | 0 |
///
/// ★ **This was never a regression** — it was true from the day the tool
/// shipped, and it was *invisible* because selection was equally blind, so
/// an operator met the page-sized form long before they met the measure
/// tool. Fixing the click in `Pass 136.0` is what made this the next wall,
/// which is the same "fixing one half exposes the other" shape this
/// subsystem has now produced three times.
///
/// # Authoring against a leaf was allowed before editing one was
///
/// A ce dimension placed against a line inside a form is a **new annotation on
/// the page**, not a change to the form, so nothing here is gated on
/// [`FormLeaf::is_editable`]. That was written when `is_editable` was a hard
/// `false`; since `Pass 188.0` a leaf that is a path *is* editable, through the
/// form-scoped geometry verbs. **This function is unaffected either way** —
/// the ungating was never a concession to the old answer, it was the
/// observation that authoring and editing are different acts.
///
/// The distinction the target carries is still needed, and for the same
/// reason: a shell has to report which list the line came from and re-resolve
/// it after an edit.
#[must_use]
pub fn pick_line_in_page(model: &PageObjects, point: Point, tolerance: f64) -> Option<PickedLine> {
    let mut best: Option<(f64, PickedLine)> = None;
    let mut consider = |candidate: Option<PickedLine>| {
        let Some(candidate) = candidate else {
            return;
        };
        // Re-measure against the segment actually chosen, so candidates are
        // compared on the same quantity `pick_line_of` used internally.
        let (dist, _) = distance_to_segment(point, candidate.start, candidate.end);
        if best.as_ref().is_none_or(|(d, _)| dist < *d) {
            best = Some((dist, candidate));
        }
    };

    for (index, obj) in model.objects.iter().enumerate() {
        // ★ A form is skipped here for free rather than by a rule: only a
        // `Path` reaches `pick_line_of` at all, and a form is an `Image`. The
        // exclusion `hit_test_point_deep` has to state explicitly is
        // structural in this function, which is why there is no `FormMarquee`
        // analogue to choose from — there is no defensible reading under
        // which a `/BBox` edge is a line the operator drew.
        let VectorObject::Path(path) = obj else {
            continue;
        };
        consider(pick_line_of(
            path,
            HitTarget::Object(index),
            point,
            tolerance,
        ));
    }
    for (index, leaf) in model.leaves.iter().enumerate() {
        let VectorObject::Path(path) = &leaf.object else {
            continue;
        };
        consider(pick_line_of(path, HitTarget::Leaf(index), point, tolerance));
    }

    best.map(|(_, line)| line)
}

/// Perpendicular distance from `p` to segment `a`–`b`, and the closest point
/// ON the segment (clamped to its ends).
fn distance_to_segment(p: Point, a: Point, b: Point) -> (f64, Point) {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let len_sq = vx.mul_add(vx, vy * vy);
    if len_sq <= f64::EPSILON {
        return ((p.x - a.x).hypot(p.y - a.y), a);
    }
    let t = (((p.x - a.x) * vx) + ((p.y - a.y) * vy)) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let proj = Point {
        x: vx.mul_add(t, a.x),
        y: vy.mul_add(t, a.y),
    };
    ((p.x - proj.x).hypot(p.y - proj.y), proj)
}

/// Decide what two picked lines mean.
///
/// # The angle returned is the one the operator pointed at
///
/// Two crossing lines bound four angles. This returns the one on the side of
/// each line where the operator actually clicked, computed by taking each
/// line's direction *oriented away from the apex toward its pick point*. That
/// is what makes clicking near the upper-left of one line and the lower-right
/// of another produce a different angle from clicking the opposite corners —
/// the behaviour SolidWorks documents and the reason [`PickedLine::pick`]
/// exists.
///
/// Returns `None` if either line is degenerate (zero length), because no
/// direction can be derived from it and every answer would be invented.
#[must_use]
pub fn classify_two_lines(
    a: &PickedLine,
    b: &PickedLine,
    policy: ParallelPolicy,
) -> Option<TwoLineRelation> {
    let (ax, ay) = a.direction()?;
    let (bx, by) = b.direction()?;

    // Angle between the two INFINITE lines, folded into [0, 90]: a line has
    // no head or tail, so a 170-degree crossing is a 10-degree one.
    let cross = ax.mul_add(by, -(ay * bx)).abs();
    let dot = ax.mul_add(bx, ay * by).abs();
    let between = cross.atan2(dot).to_degrees();

    // The operator's explicit choice outranks the measurement. Checked FIRST
    // so the threshold is not even consulted — a forced-parallel pair at 30
    // degrees must not depend on what the global epsilon happens to be.
    if policy.force_parallel || between <= policy.epsilon_degrees {
        // Parallel. The distance is from any point of one to the other's
        // infinite line; the pick point is used so a caller measuring a
        // polyline gets the distance at the place the operator was looking.
        let (dx, dy) = (b.pick.x - a.start.x, b.pick.y - a.start.y);
        let distance = dx.mul_add(ay, -(dy * ax)).abs();
        // "Apart enough to dimension" is scaled to the lines themselves
        // rather than fixed: a 2-unit gap is meaningful between two 5-unit
        // ticks and is noise between two 5000-unit construction lines.
        let scale = a.length().max(b.length()).max(1.0);
        if distance <= scale * 1e-6 {
            return Some(TwoLineRelation::Collinear);
        }
        return Some(TwoLineRelation::Parallel { distance });
    }

    let apex = infinite_intersection(a, b, ax, ay, bx, by)?;
    // Orient each line AWAY from the apex, toward where the operator clicked.
    // This is the whole mechanism by which the pick chooses the angle.
    let ua = away_from(apex, a.pick);
    let ub = away_from(apex, b.pick);
    let (ua, ub) = match (ua, ub) {
        (Some(u), Some(v)) => (u, v),
        // The operator clicked exactly at the apex on one of the lines, so
        // that line offers no side. Fall back to the segment's own direction
        // — the smallest angle — rather than refusing: a pick landing on the
        // vertex is an ordinary thing to do, and a refusal there would read
        // as the tool being broken.
        _ => {
            return Some(TwoLineRelation::Angled {
                degrees: between,
                apex,
                apex_is_real: on_segment(a, apex) && on_segment(b, apex),
            });
        }
    };
    let dot2 = ua.0.mul_add(ub.0, ua.1 * ub.1).clamp(-1.0, 1.0);
    let degrees = dot2.acos().to_degrees();

    Some(TwoLineRelation::Angled {
        degrees,
        apex,
        apex_is_real: on_segment(a, apex) && on_segment(b, apex),
    })
}

/// Unit vector from `apex` toward `pick`, or `None` if they coincide.
fn away_from(apex: Point, pick: Point) -> Option<(f64, f64)> {
    let (dx, dy) = (pick.x - apex.x, pick.y - apex.y);
    let len = dx.hypot(dy);
    if len <= f64::EPSILON {
        return None;
    }
    Some((dx / len, dy / len))
}

/// Where the two INFINITE lines cross.
fn infinite_intersection(
    a: &PickedLine,
    b: &PickedLine,
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
) -> Option<Point> {
    let denom = ax.mul_add(by, -(ay * bx));
    if denom.abs() <= f64::EPSILON {
        return None;
    }
    let (dx, dy) = (b.start.x - a.start.x, b.start.y - a.start.y);
    let t = dx.mul_add(by, -(dy * bx)) / denom;
    Some(Point {
        x: ax.mul_add(t, a.start.x),
        y: ay.mul_add(t, a.start.y),
    })
}

/// Whether `p` lies within the picked segment's own extent.
fn on_segment(line: &PickedLine, p: Point) -> bool {
    let (d, _) = distance_to_segment(p, line.start, line.end);
    d <= line.length().max(1.0) * 1e-9
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

    fn line(sx: f64, sy: f64, ex: f64, ey: f64, px: f64, py: f64) -> PickedLine {
        PickedLine {
            target: HitTarget::Object(0),
            subpath: 0,
            segment: 0,
            start: Point { x: sx, y: sy },
            end: Point { x: ex, y: ey },
            pick: Point { x: px, y: py },
        }
    }

    #[test]
    fn two_parallel_lines_give_their_perpendicular_distance() {
        let a = line(0.0, 0.0, 100.0, 0.0, 50.0, 0.0);
        let b = line(0.0, 25.0, 100.0, 25.0, 50.0, 25.0);
        match classify_two_lines(&a, &b, ParallelPolicy::default()) {
            Some(TwoLineRelation::Parallel { distance }) => {
                assert!((distance - 25.0).abs() < 1e-9, "got {distance}");
            }
            other => panic!("expected Parallel, got {other:?}"),
        }
    }

    /// Parallel does not require the two lines to overlap or be the same
    /// length — offset, staggered edges are the normal CAD case.
    #[test]
    fn parallel_works_for_staggered_lines_of_different_length() {
        let a = line(0.0, 0.0, 100.0, 0.0, 50.0, 0.0);
        let b = line(400.0, 10.0, 430.0, 10.0, 415.0, 10.0);
        match classify_two_lines(&a, &b, ParallelPolicy::default()) {
            Some(TwoLineRelation::Parallel { distance }) => {
                assert!((distance - 10.0).abs() < 1e-9, "got {distance}");
            }
            other => panic!("expected Parallel, got {other:?}"),
        }
    }

    #[test]
    fn collinear_lines_are_reported_as_such_not_as_a_zero_dimension() {
        let a = line(0.0, 0.0, 100.0, 0.0, 50.0, 0.0);
        let b = line(200.0, 0.0, 300.0, 0.0, 250.0, 0.0);
        assert_eq!(
            classify_two_lines(&a, &b, ParallelPolicy::default()),
            Some(TwoLineRelation::Collinear)
        );
    }

    #[test]
    fn perpendicular_lines_give_ninety_degrees() {
        let a = line(0.0, 0.0, 100.0, 0.0, 50.0, 0.0);
        let b = line(0.0, 0.0, 0.0, 100.0, 0.0, 50.0);
        match classify_two_lines(&a, &b, ParallelPolicy::default()) {
            Some(TwoLineRelation::Angled { degrees, .. }) => {
                assert!((degrees - 90.0).abs() < 1e-9, "got {degrees}");
            }
            other => panic!("expected Angled, got {other:?}"),
        }
    }

    /// ★ The behaviour the whole pick model exists for.
    ///
    /// Two lines crossing at the origin bound four angles. Clicking the two
    /// arms that form the 60-degree wedge must give 60; clicking one arm and
    /// the OPPOSITE arm of the other line must give its supplement, 120. An
    /// implementation that recorded only "line A, line B" would return the
    /// same number for both and could not tell these apart.
    #[test]
    fn the_pick_point_selects_which_of_the_four_angles_is_meant() {
        // Line A along +x through the origin, line B at 60 degrees.
        let a_pos = line(-100.0, 0.0, 100.0, 0.0, 50.0, 0.0);
        let a_neg = line(-100.0, 0.0, 100.0, 0.0, -50.0, 0.0);
        let (bx, by) = (60f64.to_radians().cos(), 60f64.to_radians().sin());
        let b_pos = line(
            -100.0 * bx,
            -100.0 * by,
            100.0 * bx,
            100.0 * by,
            50.0 * bx,
            50.0 * by,
        );

        let same_side = classify_two_lines(&a_pos, &b_pos, ParallelPolicy::default());
        let opposite = classify_two_lines(&a_neg, &b_pos, ParallelPolicy::default());

        let deg = |r: Option<TwoLineRelation>| match r {
            Some(TwoLineRelation::Angled { degrees, .. }) => degrees,
            other => panic!("expected Angled, got {other:?}"),
        };
        assert!(
            (deg(same_side) - 60.0).abs() < 1e-6,
            "picking both arms of the wedge must give 60, got {}",
            deg(same_side)
        );
        assert!(
            (deg(opposite) - 120.0).abs() < 1e-6,
            "picking the opposite arm must give the supplement 120, got {}",
            deg(opposite)
        );
    }

    /// Two lines that would only meet if extended still define an angle, and
    /// the apex is flagged as virtual so a shell can disclose it.
    #[test]
    fn a_virtual_apex_is_found_and_flagged() {
        let a = line(0.0, 0.0, 50.0, 0.0, 25.0, 0.0);
        let b = line(100.0, 50.0, 150.0, 100.0, 125.0, 75.0);
        match classify_two_lines(&a, &b, ParallelPolicy::default()) {
            Some(TwoLineRelation::Angled {
                apex, apex_is_real, ..
            }) => {
                assert!(!apex_is_real, "the apex is outside both segments");
                assert!(
                    (apex.y).abs() < 1e-9,
                    "the apex must sit on line A's infinite extension, got y={}",
                    apex.y
                );
            }
            other => panic!("expected Angled, got {other:?}"),
        }
    }

    /// The parallel threshold is a POLICY, and changing it changes the answer.
    ///
    /// Pins that the epsilon is actually consulted rather than being a
    /// decorative parameter — the failure mode for a settings-backed value
    /// nobody wired up.
    #[test]
    fn the_parallel_epsilon_is_consulted() {
        // Two lines 0.2 degrees apart.
        let a = line(0.0, 0.0, 100.0, 0.0, 50.0, 0.0);
        let t = 0.2f64.to_radians().tan();
        let b = line(0.0, 20.0, 100.0, 100.0f64.mul_add(t, 20.0), 50.0, 20.0);

        assert!(
            matches!(
                classify_two_lines(&a, &b, ParallelPolicy::from_setting(0.5)),
                Some(TwoLineRelation::Parallel { .. })
            ),
            "0.2 degrees apart is parallel under a 0.5 degree policy"
        );
        assert!(
            matches!(
                classify_two_lines(&a, &b, ParallelPolicy::from_setting(0.1)),
                Some(TwoLineRelation::Angled { .. })
            ),
            "the same pair is ANGLED under a 0.1 degree policy"
        );
    }

    /// A zero-length line yields no answer rather than a made-up one.
    #[test]
    fn a_degenerate_line_is_refused() {
        let a = line(10.0, 10.0, 10.0, 10.0, 10.0, 10.0);
        let b = line(0.0, 0.0, 100.0, 0.0, 50.0, 0.0);
        assert_eq!(classify_two_lines(&a, &b, ParallelPolicy::default()), None);
    }
}

/// Tests for the page-wide picker, over genuinely decomposed content streams
/// rather than hand-built [`PickedLine`]s — this is the layer where "which
/// object did the click land on" is actually decided.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod page_pick_tests {
    use super::*;
    use crate::content::ContentStream;
    use crate::vector::decompose::{NoXObjects, decompose};
    use crate::vector::geometry::Matrix;

    fn model(src: &[u8]) -> PageObjects {
        let cs = ContentStream::parse(src.to_vec()).unwrap();
        decompose(&cs, Matrix::IDENTITY, &NoXObjects)
    }

    /// A click near one of two separate stroked lines picks THAT line, and
    /// reports which object it came from.
    #[test]
    fn a_click_picks_the_nearest_line_across_objects() {
        // Two horizontal strokes as two separate path objects.
        let m = model(b"10 100 m 200 100 l S 10 300 m 200 300 l S");
        let picked = pick_line_in_page(&m, Point { x: 100.0, y: 302.0 }, 5.0)
            .expect("a click 2 units from the upper line must pick it");
        assert_eq!(
            picked.target,
            HitTarget::Object(1),
            "the SECOND path is the near one"
        );
        assert!((picked.start.y - 300.0).abs() < 1e-9);
        assert!(
            (picked.pick.x - 100.0).abs() < 1e-9,
            "the pick projects onto the segment at the click's x, got {:?}",
            picked.pick
        );
    }

    /// ★ The nearest line wins even when a farther object is drawn first —
    /// the failure this guards is a pick that resolves by content-stream order
    /// and therefore looks random to the operator.
    #[test]
    fn paint_order_does_not_decide_the_pick() {
        // The FAR line is drawn first, the NEAR line second.
        let m = model(b"0 0 m 200 0 l S 0 100 m 200 100 l S");
        let picked =
            pick_line_in_page(&m, Point { x: 50.0, y: 96.0 }, 10.0).expect("within tolerance");
        assert_eq!(
            picked.target,
            HitTarget::Object(1),
            "the nearer line must win regardless of which was painted first"
        );
    }

    /// Nothing within tolerance yields no pick, rather than the least-bad line
    /// on the page.
    #[test]
    fn a_click_in_empty_space_picks_nothing() {
        let m = model(b"10 100 m 200 100 l S");
        assert!(pick_line_in_page(&m, Point { x: 100.0, y: 400.0 }, 5.0).is_none());
    }

    /// ★ A curve is not a line, and clicking one picks nothing rather than its
    /// chord. Dimensioning "the line" of a Bézier would measure something the
    /// drawing does not contain.
    #[test]
    fn clicking_a_curve_picks_nothing() {
        let m = model(b"0 0 m 50 100 150 100 200 0 c S");
        assert!(
            pick_line_in_page(&m, Point { x: 100.0, y: 74.0 }, 12.0).is_none(),
            "a cubic must be skipped, never chorded"
        );
    }

    /// A rectangle's four edges are individually pickable, and clicking near
    /// one picks that edge rather than the whole shape.
    #[test]
    fn an_edge_of_a_rectangle_is_pickable_on_its_own() {
        let m = model(b"100 100 200 50 re S");
        // Near the bottom edge (y = 100).
        let bottom =
            pick_line_in_page(&m, Point { x: 200.0, y: 102.0 }, 5.0).expect("the bottom edge");
        assert!((bottom.start.y - 100.0).abs() < 1e-9 && (bottom.end.y - 100.0).abs() < 1e-9);
        // Near the top edge (y = 150).
        let top = pick_line_in_page(&m, Point { x: 200.0, y: 148.0 }, 5.0).expect("the top edge");
        assert!((top.start.y - 150.0).abs() < 1e-9 && (top.end.y - 150.0).abs() < 1e-9);
    }

    /// ★ End-to-end: two edges of one rectangle, picked by clicking, classify
    /// the way the operator expects — the opposite edges are parallel and the
    /// adjacent ones meet at a right angle. This is the whole feature in
    /// miniature, driven only by click coordinates.
    #[test]
    fn two_clicks_on_a_rectangle_classify_as_the_geometry_demands() {
        let m = model(b"100 100 200 50 re S");
        let bottom = pick_line_in_page(&m, Point { x: 200.0, y: 101.0 }, 5.0).expect("bottom");
        let top = pick_line_in_page(&m, Point { x: 200.0, y: 149.0 }, 5.0).expect("top");
        let left = pick_line_in_page(&m, Point { x: 101.0, y: 125.0 }, 5.0).expect("left");

        match classify_two_lines(&bottom, &top, ParallelPolicy::default()) {
            Some(TwoLineRelation::Parallel { distance }) => {
                assert!(
                    (distance - 50.0).abs() < 1e-6,
                    "the two long edges are 50 apart, got {distance}"
                );
            }
            other => panic!("opposite edges must be parallel, got {other:?}"),
        }
        match classify_two_lines(&bottom, &left, ParallelPolicy::default()) {
            Some(TwoLineRelation::Angled { degrees, .. }) => {
                assert!(
                    (degrees - 90.0).abs() < 1e-6,
                    "adjacent edges meet at a right angle, got {degrees}"
                );
            }
            other => panic!("adjacent edges must be angled, got {other:?}"),
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod override_tests {
    use super::*;

    fn line(sx: f64, sy: f64, ex: f64, ey: f64, px: f64, py: f64) -> PickedLine {
        PickedLine {
            target: HitTarget::Object(0),
            subpath: 0,
            segment: 0,
            start: Point { x: sx, y: sy },
            end: Point { x: ex, y: ey },
            pick: Point { x: px, y: py },
        }
    }

    /// ★ The operator's checkbox beats the measurement, by a wide margin.
    ///
    /// Two lines 30 degrees apart are unambiguously angled by every automatic
    /// reading. Ticking "treat as parallel" must still produce a parallel
    /// relation — otherwise the checkbox is advisory, which is the one thing
    /// an override must never be.
    #[test]
    fn forcing_parallel_beats_the_measurement_entirely() {
        let a = line(0.0, 0.0, 100.0, 0.0, 50.0, 0.0);
        let t = 30f64.to_radians().tan();
        let b = line(0.0, 40.0, 100.0, 100.0f64.mul_add(t, 40.0), 50.0, 40.0);

        assert!(
            matches!(
                classify_two_lines(&a, &b, ParallelPolicy::from_setting(0.5)),
                Some(TwoLineRelation::Angled { .. })
            ),
            "without the override this pair is plainly angled"
        );
        assert!(
            matches!(
                classify_two_lines(&a, &b, ParallelPolicy::from_setting(0.5).forcing_parallel()),
                Some(TwoLineRelation::Parallel { .. })
            ),
            "with the operator's checkbox ticked it must be parallel"
        );
    }

    /// The override does not depend on the global threshold at all.
    ///
    /// Pins that `force_parallel` short-circuits BEFORE the epsilon is read.
    /// A build that ANDed the two would pass the test above (0.5 is a normal
    /// value) and fail here, which is why this case is separate.
    #[test]
    fn forcing_parallel_ignores_the_threshold_even_at_zero() {
        let a = line(0.0, 0.0, 100.0, 0.0, 50.0, 0.0);
        let b = line(0.0, 10.0, 100.0, 60.0, 50.0, 35.0);
        assert!(
            matches!(
                classify_two_lines(
                    &a,
                    &b,
                    ParallelPolicy {
                        epsilon_degrees: 0.0,
                        force_parallel: true
                    }
                ),
                Some(TwoLineRelation::Parallel { .. })
            ),
            "a zero threshold must not defeat an explicit override"
        );
    }

    /// The override does not fake the measurement — the real angle stays
    /// readable, so a shell can disclose what is being overridden.
    #[test]
    fn the_real_angle_remains_available_for_disclosure() {
        let a = line(0.0, 0.0, 100.0, 0.0, 50.0, 0.0);
        let t = 30f64.to_radians().tan();
        let b = line(0.0, 40.0, 100.0, 100.0f64.mul_add(t, 40.0), 50.0, 40.0);
        let measured = measured_angle_degrees(&a, &b).expect("both lines are real");
        assert!(
            (measured - 30.0).abs() < 1e-6,
            "the measured angle must be reported truthfully, got {measured}"
        );
    }

    /// A forced-parallel pair still reports a real distance, measured at the
    /// pick point — not a fabricated one.
    #[test]
    fn a_forced_parallel_pair_reports_the_distance_at_the_pick() {
        let a = line(0.0, 0.0, 100.0, 0.0, 50.0, 0.0);
        // Diverging line, 40 units above A at the picked x.
        let t = 10f64.to_radians().tan();
        let b = line(
            0.0,
            40.0,
            100.0,
            100.0f64.mul_add(t, 40.0),
            50.0,
            50.0f64.mul_add(t, 40.0),
        );
        match classify_two_lines(&a, &b, ParallelPolicy::from_setting(0.5).forcing_parallel()) {
            Some(TwoLineRelation::Parallel { distance }) => {
                let expected = 50.0f64.mul_add(t, 40.0);
                assert!(
                    (distance - expected).abs() < 1e-6,
                    "expected the perpendicular distance at the pick ({expected}), got {distance}"
                );
            }
            other => panic!("expected Parallel, got {other:?}"),
        }
    }
}

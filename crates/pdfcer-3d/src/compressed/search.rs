//! The planar-face check, and the last-resort search for a mesh no walk
//! fits.

use std::collections::HashMap;

use super::normals::NormalArrays;
use super::{
    Arrays, MAX_BEND, Mark, NormalReader, TriangleMesh, V, Walk, add, cross, dot, mesh, mul, sub,
    unit,
};

/// How many inverted choices one search combines.
const DEPTH: usize = 2;
/// How many a deeper search combines when no shallow one fits.
const DEEP: usize = 4;
/// How many the last search combines when no other fits.
const DEEPEST: usize = 6;
/// The searches [`unique_fit`] runs in turn: whether turn candidates are
/// offered, and how many choices are combined.
pub(super) const STAGES: [(bool, usize); 4] =
    [(false, DEPTH), (true, DEPTH), (true, DEEP), (true, DEEPEST)];
/// How many unsignalled folds and turns before a failure are tried.
const PRIMARIES: usize = 40;
/// How many signalled folds among them are tried. Fewer than
/// [`PRIMARIES`]: a run of weak signalled triangles would otherwise fill
/// the window and hide the one unsignalled sliver whose fold is wrong.
const SECONDARIES: usize = 20;
/// Triangle steps a search may spend before giving up undecided.
const STEP_BUDGET: usize = 2_000_000;
/// How many inverted choices the best-fit search combines.
const BEST_DEPTH: usize = 64;
/// Triangle steps the best-fit search spends on each reading.
const BEST_BUDGET: usize = 250_000;
/// How many triangles before a failure [`events`] looks for candidates.
const EVENT_WINDOW: usize = 300;
/// Triangle steps [`events`] may spend.
const EVENT_BUDGET: usize = 1_000_000;

/// Whether every planar face among `tris[from..]` lies within
/// [`MAX_BEND`] tolerances of the plane through its centroid, oriented by
/// the sum of its triangles' normals. True for a mesh without stored
/// normals, which names no planar faces.
pub(super) fn flat(a: &Arrays<'_>, pos: &[V], tris: &[[u32; 3]], from: usize) -> bool {
    let Some(n) = a.normals.as_ref() else {
        return true;
    };
    let mut faces: HashMap<u32, Vec<[u32; 3]>> = HashMap::new();
    for (i, t) in tris.iter().enumerate().skip(from) {
        let Some(&f) = n.face_of.get(i) else { break };
        if n.planar.get(f as usize).copied().unwrap_or(false) {
            faces.entry(f).or_default().push(*t);
        }
    }
    let at = |v: u32| pos.get(v as usize).copied().unwrap_or_default();
    faces.values().all(|ts| {
        let (mut normal, mut centre) = ([0.0; 3], [0.0; 3]);
        for t in ts {
            let (p, q, r) = (at(t[0]), at(t[1]), at(t[2]));
            let w = cross(sub(q, p), sub(r, p));
            normal = add(
                normal,
                if dot(w, normal) < 0.0 {
                    mul(w, -1.0)
                } else {
                    w
                },
            );
            centre = add(centre, add(p, add(q, r)));
        }
        if dot(normal, normal) == 0.0 {
            return true;
        }
        #[allow(clippy::cast_precision_loss)] // A face's triangle count.
        let centre = mul(centre, 1.0 / (3 * ts.len()) as f64);
        let normal = unit(normal);
        ts.iter()
            .flatten()
            .all(|&v| dot(sub(at(v), centre), normal).abs() <= MAX_BEND * a.tolerance)
    })
}

/// One inverted decision at a triangle: its fold, or the half-turn of its
/// degenerate apex frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Fold(usize),
    Turn(usize),
}

impl Choice {
    fn at(self) -> usize {
        match self {
            Choice::Fold(i) | Choice::Turn(i) => i,
        }
    }
}

/// What one walk with a fixed set of inverted choices came to.
enum Outcome {
    Fit(TriangleMesh),
    /// Failed at a triangle; the candidates before it, in its
    /// component and after the last inversion, nearest last.
    Failed(Vec<Choice>),
    /// Failed in a way no further inversion can repair.
    Dead,
}

/// Searches combinations of inverted choices for one that rebuilds the
/// mesh. A fold candidate is a triangle no stored decision orients, or a
/// sliver at most a tolerance from collinear, whose decoded decision is
/// noise; a turn candidate is an apex whose frame is
/// degenerate, whose turn rests on a rounding residue. A fit is one that
/// rebuilds the mesh: every array and stored normal consumed exactly and
/// every planar face flat. The rebuild is returned only if it is the
/// single such geometry the search finds within [`STEP_BUDGET`]; two, or a
/// search cut short, would make it a guess. Each of [`STAGES`] runs only
/// when the one before found no fit at all: folds alone, then turns too,
/// then [`DEEP`] and [`DEEPEST`] choices, so a shallower rebuild is never
/// second-guessed.
pub(super) fn unique_fit(a: &Arrays<'_>) -> Option<TriangleMesh> {
    unique_fit_reading(a).0
}

/// [`unique_fit`] on `a`, then on `turned`, the same arrays read with
/// [`Arrays::ortho_turned`] flipped. The second search is skipped when no
/// walk of the first met an apex the flip moves: it would repeat the first
/// step for step.
pub(super) fn unique_fit_either(a: &Arrays<'_>, turned: &Arrays<'_>) -> Option<TriangleMesh> {
    match unique_fit_reading(a) {
        (Some(m), _) => Some(m),
        (None, true) => unique_fit(turned),
        (None, false) => None,
    }
}

/// [`unique_fit`], and whether any of its walks read [`Arrays::ortho_turned`].
pub(super) fn unique_fit_reading(a: &Arrays<'_>) -> (Option<TriangleMesh>, bool) {
    let Some(n) = a.normals.as_ref() else {
        return (None, false);
    };
    let mut reads_turned = false;
    for (turns, depth) in STAGES {
        let (found, reads) = search_reading(a, n, Plan::unique(turns, depth));
        reads_turned |= reads;
        match found {
            Search::Unique(m) => return (Some(*m), reads_turned),
            Search::Refused => return (None, reads_turned),
            Search::NoFit => {}
        }
    }
    (None, reads_turned)
}

/// Whether a fold-only search rebuilds `a`.
#[cfg(test)]
pub(super) fn fits_without_turns(a: &Arrays<'_>) -> bool {
    a.normals
        .as_ref()
        .is_some_and(|n| matches!(search(a, n, Plan::unique(false, DEPTH)), Search::Unique(_)))
}

/// Whether a search of at most `depth` choices, turns included, rebuilds
/// `a`.
#[cfg(test)]
pub(super) fn fits_within(a: &Arrays<'_>, depth: usize) -> bool {
    a.normals
        .as_ref()
        .is_some_and(|n| matches!(search(a, n, Plan::unique(true, depth)), Search::Unique(_)))
}

/// The first rebuild a deep search finds, for a mesh [`unique_fit`]
/// leaves unbuilt. Measured on real producers, an apex quantised exactly
/// onto its edge's line (`d.y == d.z == 0`) keeps no trace of its fold,
/// so a mesh with several needs more inverted choices than a unique
/// search can afford to rule out, and may fit more than one way. Up to
/// [`BEST_DEPTH`] choices are combined, within [`BEST_BUDGET`] steps per
/// reading: the arrays as read, then [`Arrays::legacy_orient`], then with
/// slivers oriented by their corner normals ([`Walk::raw`]), then with a
/// closed continuation edge overriding a stored normal
/// ([`Walk::open_wins`]), then [`greedy`], then [`events`]. Measured on real producers, a normal can send
/// the walk onto an edge two triangles already share, which no choice
/// after it can repair. The result
/// consumes every array and stored normal and keeps every planar face
/// flat, but another geometry may do the same; callers disclose it.
pub(super) fn best_fit(a: &Arrays<'_>) -> Option<TriangleMesh> {
    let legacy = Arrays {
        legacy_orient: true,
        ..*a
    };
    [
        (a, false, false),
        (&legacy, false, false),
        (a, true, false),
        (a, false, true),
        (&legacy, false, true),
    ]
    .into_iter()
    .find_map(|(a, raw, open)| {
        let plan = Plan {
            turns: true,
            depth: BEST_DEPTH,
            budget: BEST_BUDGET,
            raw,
            open,
            first: true,
        };
        match search(a, a.normals.as_ref()?, plan) {
            Search::Unique(m) => Some(*m),
            Search::Refused | Search::NoFit => None,
        }
    })
    .or_else(|| greedy(a, a.normals.as_ref()?))
    .or_else(|| events(a, a.normals.as_ref()?).map(|(_, m)| m))
}

/// The last best-fit reading: a best-first search over choice sets,
/// always expanding the one whose walk reaches furthest (fewer choices
/// first among equals). A set is expanded by trial-walking each
/// unsignalled fold and turn within [`EVENT_WINDOW`] triangles before its
/// failure and after its last choice; a candidate whose walk fails at the
/// same triangle is dropped as inert.
///
/// Measured on real producers: a wrong sliver fold can survive some 250
/// triangles before the walk fails, past any window [`window`] keeps, and
/// several such folds may be needed jointly; most slivers in a window are
/// inert, and enumerating their subsets is what exhausted the depth-first
/// searches. Two jointly-needed choices that are each inert alone are
/// missed; that is accepted.
fn events(a: &Arrays<'_>, n: &NormalArrays<'_>) -> Option<(Vec<Choice>, TriangleMesh)> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let mut walker = Resume::new(a, n, SNAP_EVERY, false);
    if let Outcome::Fit(m) = walker.attempt(a, n, &[]) {
        return Some((Vec::new(), m));
    }
    // Per set: its choices and the first triangle a new choice may take.
    let mut sets: Vec<(Vec<Choice>, usize)> = vec![(Vec::new(), 0)];
    let mut heap = BinaryHeap::from([(walker.steps.len(), Reverse(0), 0)]);
    while let Some((reach, _, id)) = heap.pop() {
        if walker.spent > EVENT_BUDGET {
            return None;
        }
        let (flips, decided) = sets.get(id)?.clone();
        walker.attempt(a, n, &flips);
        for c in event_candidates(&walker.steps, decided, reach) {
            let mut g = flips.clone();
            g.push(c);
            if let Outcome::Fit(m) = walker.attempt(a, n, &g) {
                return Some((g, m));
            }
            let r = walker.steps.len();
            if r != reach {
                heap.push((r, Reverse(g.len()), sets.len()));
                sets.push((g, c.at() + 1));
            }
        }
    }
    None
}

/// The last best-fit reading: one choice set grown a choice at a time,
/// each round adding the candidate whose walk gets furthest, with
/// [`Walk::open_wins`] and [`Walk::loose`] candidates. A depth-first
/// search spends its budget under a wrong first choice; measured on real
/// producers, the furthest-reaching choice is the needed one at each
/// failure, so this fits meshes whose choices lie far apart.
fn greedy(a: &Arrays<'_>, n: &NormalArrays<'_>) -> Option<TriangleMesh> {
    let mut walker = Resume::new(a, n, SNAP_EVERY, false);
    walker.w.open_wins = true;
    walker.w.loose = true;
    let mut flips = Vec::new();
    let mut candidates = match walker.attempt(a, n, &flips) {
        Outcome::Fit(m) => return Some(m),
        Outcome::Failed(c) => c,
        Outcome::Dead => return None,
    };
    let mut reached = walker.steps.len();
    while walker.spent < BEST_BUDGET {
        let mut best: Option<(usize, Choice, Vec<Choice>)> = None;
        for j in candidates {
            let mut g = flips.clone();
            g.push(j);
            match walker.attempt(a, n, &g) {
                Outcome::Fit(m) => return Some(m),
                Outcome::Failed(c) => {
                    let r = walker.steps.len();
                    if r > best.as_ref().map_or(reached, |b| b.0) {
                        best = Some((r, j, c));
                    }
                }
                Outcome::Dead => {}
            }
        }
        let (r, j, c) = best?;
        flips.push(j);
        (reached, candidates) = (r, c);
    }
    None
}

/// The choices [`events`] tries after a failure at `reach`: each turn and
/// unsignalled fold within [`EVENT_WINDOW`] triangles before it and at or
/// after `decided`, nearest first.
fn event_candidates(steps: &[Step], decided: usize, reach: usize) -> Vec<Choice> {
    let from = decided.max(reach.saturating_sub(EVENT_WINDOW));
    let mut c = Vec::new();
    for (i, s) in steps.iter().enumerate().take(reach).skip(from).rev() {
        if s.fold && !s.signalled {
            c.push(Choice::Fold(i));
        }
        if s.turn {
            c.push(Choice::Turn(i));
        }
    }
    c
}

/// How one search runs.
#[derive(Clone, Copy)]
struct Plan {
    /// Whether turn candidates are offered.
    turns: bool,
    /// How many inverted choices are combined.
    depth: usize,
    budget: usize,
    /// Whether slivers are oriented by their corner normals.
    raw: bool,
    /// Whether a closed continuation edge overrides a stored normal.
    open: bool,
    /// Whether the first fit is returned without ruling out a second.
    first: bool,
}

impl Plan {
    fn unique(turns: bool, depth: usize) -> Self {
        Plan {
            turns,
            depth,
            budget: STEP_BUDGET,
            raw: false,
            open: false,
            first: false,
        }
    }
}

/// What one search came to.
enum Search {
    Unique(Box<TriangleMesh>),
    /// Two geometries fit, or the budget ran out after a fit.
    Refused,
    NoFit,
}

/// One depth-first search as `plan` says. Each attempt resumes the last
/// one's walk where their choices first differ, so the budget counts
/// triangle steps actually taken.
fn search(a: &Arrays<'_>, n: &NormalArrays<'_>, plan: Plan) -> Search {
    search_reading(a, n, plan).0
}

/// [`search`], and whether any of its walks read [`Arrays::ortho_turned`].
fn search_reading(a: &Arrays<'_>, n: &NormalArrays<'_>, plan: Plan) -> (Search, bool) {
    let mut walker = Resume::new(a, n, SNAP_EVERY, plan.raw);
    walker.w.open_wins = plan.open;
    let mut stack = vec![Vec::new()];
    let mut found: Option<TriangleMesh> = None;
    while walker.spent < plan.budget {
        let Some(flips) = stack.pop() else {
            let found = found.map_or(Search::NoFit, |m| Search::Unique(Box::new(m)));
            return (found, walker.w.reads_turned);
        };
        match walker.attempt(a, n, &flips) {
            Outcome::Fit(m) => match &found {
                Some(f) if f.triangles != m.triangles || f.positions != m.positions => {
                    return (Search::Refused, walker.w.reads_turned);
                }
                _ if plan.first => {
                    return (Search::Unique(Box::new(m)), walker.w.reads_turned);
                }
                _ => found = Some(m),
            },
            Outcome::Failed(c) if flips.len() < plan.depth => {
                for j in c {
                    if plan.turns || matches!(j, Choice::Fold(_)) {
                        let mut g = flips.clone();
                        g.push(j);
                        stack.push(g);
                    }
                }
            }
            Outcome::Failed(_) | Outcome::Dead => {}
        }
    }
    let found = if found.is_some() {
        Search::Refused
    } else {
        Search::NoFit
    };
    (found, walker.w.reads_turned)
}

/// Triangles between the snapshots a resumed walk rewinds to.
const SNAP_EVERY: usize = 64;

/// What one completed step of a walk offers the search.
#[derive(Clone, Copy)]
struct Step {
    /// Whether the step began a component.
    start: bool,
    /// Whether its fold is a candidate.
    fold: bool,
    /// Whether its record signalled the fold, so the candidate is a
    /// secondary one.
    signalled: bool,
    /// Whether its turn is a candidate.
    turn: bool,
}

impl Step {
    /// The step a walk just took.
    fn of(w: &Walk, start: bool) -> Self {
        Step {
            start,
            fold: !w.signalled || w.weak || (w.loose && !w.exact),
            signalled: w.signalled,
            turn: w.degenerate,
        }
    }
}

/// The candidates a search tries after a failure, from `back`: the steps
/// before it, nearest first, within its component and after the last
/// inversion. The last [`PRIMARIES`] unsignalled folds and turns, and at
/// most [`SECONDARIES`] signalled folds among them, nearest last.
fn window<'a>(back: impl Iterator<Item = (usize, &'a Step)>) -> Vec<Choice> {
    let (mut primaries, mut secondaries) = (0, 0);
    let mut c = Vec::new();
    for (i, s) in back {
        if s.turn {
            c.push(Choice::Turn(i));
            primaries += 1;
        }
        if s.fold && !s.signalled {
            c.push(Choice::Fold(i));
            primaries += 1;
        }
        if s.fold && s.signalled && secondaries < SECONDARIES {
            c.push(Choice::Fold(i));
            secondaries += 1;
        }
        if primaries >= PRIMARIES || s.start {
            break;
        }
    }
    c.reverse();
    c
}

/// The walk state before a triangle, beyond what a [`Mark`] holds.
struct Snap {
    mark: Mark,
    stack: Vec<(u32, u32, u32)>,
    next: Option<(u32, u32, u32)>,
    sliver: Option<usize>,
}

/// One walk kept across a search's attempts. Its state before triangle
/// `i` depends only on the choices at triangles before `i`, so an attempt
/// rewinds to the last snapshot at or before the first choice it does not
/// share with the previous attempt and walks on from there.
struct Resume<'a> {
    w: Walk<'a>,
    flips: Vec<Choice>,
    steps: Vec<Step>,
    snaps: Vec<Snap>,
    /// Triangles between snapshots.
    every: usize,
    /// Triangle steps taken so far.
    spent: usize,
}

impl<'a> Resume<'a> {
    fn new(a: &Arrays<'_>, n: &'a NormalArrays<'a>, every: usize, raw: bool) -> Self {
        let mut w = Walk::new(a.triangles, Some(NormalReader::new(n)));
        w.raw = raw;
        let snap = Snap {
            mark: w.mark(0),
            stack: Vec::new(),
            next: None,
            sliver: None,
        };
        Resume {
            w,
            flips: Vec::new(),
            steps: Vec::new(),
            snaps: vec![snap],
            every: every.max(1),
            spent: 0,
        }
    }

    /// Walks the whole mesh oriented, inverting each of `flips`, with no
    /// retries; the same outcome as a walk from the first triangle.
    fn attempt(&mut self, a: &Arrays<'_>, n: &NormalArrays<'_>, flips: &[Choice]) -> Outcome {
        let differ = self
            .flips
            .iter()
            .filter(|c| !flips.contains(c))
            .chain(flips.iter().filter(|c| !self.flips.contains(c)))
            .map(|c| c.at())
            .min()
            .unwrap_or(usize::MAX);
        self.rewind(differ.min(self.steps.len()));
        self.flips = flips.to_vec();
        let after = flips.iter().map(|c| c.at() + 1).max().unwrap_or(0);
        for i in self.steps.len()..a.triangles {
            if i % self.every == 0 && self.snaps.last().is_some_and(|s| s.mark.tri < i) {
                self.snaps.push(Snap {
                    mark: self.w.mark(i),
                    stack: self.w.stack.clone(),
                    next: self.w.next,
                    sliver: self.w.sliver,
                });
            }
            let start = self.w.next.is_none();
            let status = a.edge_status.get(i).copied().unwrap_or(0);
            self.w.turn = flips.contains(&Choice::Turn(i));
            self.spent += 1;
            if self
                .w
                .step(a, status, flips.contains(&Choice::Fold(i)))
                .is_err()
            {
                return Outcome::Failed(if start {
                    Vec::new()
                } else {
                    self.candidates(i, after)
                });
            }
            self.steps.push(Step::of(&self.w, start));
        }
        let w = &self.w;
        if w.slot != a.is_reference.len() || w.ri != a.references.len() || w.pi != a.points.len() {
            return Outcome::Dead;
        }
        self.spent += a.triangles;
        attempt(a, n, flips, self.w.raw, self.w.open_wins)
    }

    /// Restores the walk to the last snapshot at or before triangle `at`.
    fn rewind(&mut self, at: usize) {
        while self.snaps.len() > 1 && self.snaps.last().is_some_and(|s| s.mark.tri > at) {
            self.snaps.pop();
        }
        let Some(s) = self.snaps.last() else { return };
        self.w.rewind(s.mark);
        self.w.stack.clone_from(&s.stack);
        (self.w.next, self.w.sliver) = (s.next, s.sliver);
        self.steps.truncate(s.mark.tri);
    }

    /// The [`window`] of candidates before a failure at `failed`, in its
    /// component and from `after` on.
    fn candidates(&self, failed: usize, after: usize) -> Vec<Choice> {
        window(
            (after..failed)
                .rev()
                .map_while(|i| self.steps.get(i).map(|s| (i, s))),
        )
    }
}

/// Walks the whole mesh oriented, inverting each of `flips`, with no
/// retries; `raw` and `open` as [`Walk::raw`] and [`Walk::open_wins`].
fn attempt(
    a: &Arrays<'_>,
    n: &NormalArrays<'_>,
    flips: &[Choice],
    raw: bool,
    open: bool,
) -> Outcome {
    let mut w = Walk::new(a.triangles, Some(NormalReader::new(n)));
    w.raw = raw;
    w.open_wins = open;
    let after = flips.iter().map(|c| c.at() + 1).max().unwrap_or(0);
    let mut steps = Vec::new();
    for i in 0..a.triangles {
        let start = w.next.is_none();
        let status = a.edge_status.get(i).copied().unwrap_or(0);
        w.turn = flips.contains(&Choice::Turn(i));
        if w.step(a, status, flips.contains(&Choice::Fold(i))).is_err() {
            let back = steps.iter().enumerate().rev();
            return Outcome::Failed(if start {
                Vec::new()
            } else {
                window(back.take_while(|&(j, _)| j >= after))
            });
        }
        steps.push(Step::of(&w, start));
    }
    if w.slot != a.is_reference.len() || w.ri != a.references.len() || w.pi != a.points.len() {
        return Outcome::Dead;
    }
    let Walk {
        pos, tris, normals, ..
    } = w;
    match normals.and_then(|r| r.finish(&pos)) {
        Some(stored) if flat(a, &pos, &tris, 0) => Outcome::Fit(mesh(pos, tris, stored)),
        _ => Outcome::Dead,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)] // Tests fail loudly.
mod tests {
    use super::*;

    /// Edge statuses, points, slots, references and normal angles.
    type Case<'a> = (&'a [i32], Vec<i64>, usize, &'a [u32], &'a [i32]);

    /// A comparable digest of an outcome.
    fn digest(o: Outcome) -> (u8, Vec<Choice>, Vec<[u32; 3]>, Vec<V>) {
        match o {
            Outcome::Fit(m) => (0, Vec::new(), m.triangles, m.positions),
            Outcome::Failed(c) => (1, c, Vec::new(), Vec::new()),
            Outcome::Dead => (2, Vec::new(), Vec::new(), Vec::new()),
        }
    }

    /// A step whose fold is a candidate.
    fn fold_step(signalled: bool) -> Step {
        Step {
            start: false,
            fold: true,
            signalled,
            turn: false,
        }
    }

    /// One unsignalled fold followed by a run of signalled ones longer
    /// than the window: the window keeps the unsignalled fold and the
    /// nearest [`SECONDARIES`] signalled ones, so a run of weak triangles
    /// cannot hide the one sliver whose fold is wrong.
    #[test]
    fn a_run_of_signalled_folds_does_not_push_an_unsignalled_one_out() {
        let mut steps = vec![fold_step(false)];
        steps.extend((0..PRIMARIES + SECONDARIES).map(|_| fold_step(true)));
        let c = window(steps.iter().enumerate().rev());
        let n = steps.len();
        assert_eq!(c.len(), SECONDARIES + 1);
        assert_eq!(c[0], Choice::Fold(0));
        assert_eq!(c[1], Choice::Fold(n - SECONDARIES));
        assert_eq!(c[SECONDARIES], Choice::Fold(n - 1));
        let mut steps = vec![fold_step(false); PRIMARIES + 1];
        steps[0].start = true;
        assert_eq!(window(steps.iter().enumerate().rev()).len(), PRIMARIES);
        steps.truncate(3);
        assert_eq!(window(steps.iter().enumerate().rev()).len(), 3);
    }

    #[test]
    fn event_candidates_are_unsignalled_folds_and_turns_in_the_window() {
        let mut steps = vec![fold_step(false); EVENT_WINDOW + 10];
        steps[EVENT_WINDOW + 8].signalled = true;
        steps[EVENT_WINDOW + 7].turn = true;
        let reach = EVENT_WINDOW + 9;
        let c = event_candidates(&steps, 0, reach);
        assert_eq!(c.len(), EVENT_WINDOW);
        assert_eq!(c[0], Choice::Fold(EVENT_WINDOW + 7));
        assert_eq!(c[1], Choice::Turn(EVENT_WINDOW + 7));
        assert_eq!(c.last(), Some(&Choice::Fold(9)));
        let c = event_candidates(&steps, EVENT_WINDOW + 7, reach);
        assert_eq!(
            c,
            [
                Choice::Fold(EVENT_WINDOW + 7),
                Choice::Turn(EVENT_WINDOW + 7)
            ]
        );
        assert!(event_candidates(&steps, 0, 0).is_empty());
    }

    /// Every set of at most two choices over `a`'s triangles, in an order
    /// that moves the first differing choice both ways between attempts.
    fn flip_sets(t: usize) -> Vec<Vec<Choice>> {
        let all: Vec<Choice> = (0..t)
            .flat_map(|i| [Choice::Fold(i), Choice::Turn(i)])
            .collect();
        let mut sets = vec![Vec::new()];
        for (k, &x) in all.iter().enumerate() {
            sets.push(vec![x]);
            for &y in all.iter().skip(k + 1) {
                sets.push(vec![x, y]);
            }
        }
        let back: Vec<_> = sets.iter().rev().cloned().collect();
        sets.extend(back);
        sets
    }

    /// A resumed attempt comes to exactly what a walk from the first
    /// triangle does, whatever attempt preceded it, with a snapshot at
    /// every triangle or only the first.
    #[test]
    fn a_resumed_attempt_matches_a_fresh_walk() {
        let tri0 = [0, 0, 0, 4, 0, 0, 2, 4, 0];
        let cases: [Case<'_>; 2] = [
            (
                &[1, 3, 1, 1],
                [&tri0[..], &[0, 0, 0, -4, -4, 2]].concat(),
                6,
                &[2],
                &[470, 724],
            ),
            (
                &[1, 3, 2, 1, 2],
                [&tri0[..], &[-4, 0, 0, 3, 3, 3, -1, 0, 0]].concat(),
                7,
                &[0],
                &[837, 402],
            ),
        ];
        for (status, pts, slots, refs, angles) in cases {
            let t = status.len();
            let mut is_ref = vec![false; slots];
            is_ref[5] = true;
            let face_of = vec![0; t];
            for legacy in [false, true] {
                let a = Arrays {
                    tolerance: 0.5,
                    origin: [10.0, 0.0, 0.0],
                    points: &pts,
                    edge_status: status,
                    triangles: t,
                    is_reference: &is_ref,
                    references: refs,
                    ortho_turned: false,
                    legacy_orient: legacy,
                    normals: Some(NormalArrays {
                        bits: 10,
                        binary: &[false; 4],
                        angles,
                        planar: &[true],
                        face_of: &face_of,
                    }),
                };
                let n = a.normals.as_ref().unwrap();
                for every in [1, SNAP_EVERY] {
                    let mut r = Resume::new(&a, n, every, false);
                    let mut kinds = [0; 3];
                    for flips in flip_sets(t) {
                        let got = digest(r.attempt(&a, n, &flips));
                        let want = digest(attempt(&a, n, &flips, false, false));
                        kinds[usize::from(want.0)] += 1;
                        assert!(
                            got == want,
                            "flips {:?}",
                            flips.iter().map(|c| c.at()).collect::<Vec<_>>()
                        );
                    }
                    assert!(kinds[0] > 0 && kinds[1] > 0, "{kinds:?}");
                }
            }
        }
    }

    /// The same on seeded arbitrary arrays, valid or not, whose walks
    /// branch, pop stacked edges and start new components.
    #[test]
    fn a_resumed_attempt_matches_a_fresh_walk_on_arbitrary_arrays() {
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = |m: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % m
        };
        let mut kinds = [0; 3];
        for _ in 0..300 {
            let t = 6 + usize::try_from(next(5)).unwrap();
            let status: Vec<i32> = (0..t).map(|_| i32::try_from(next(4)).unwrap()).collect();
            let pts: Vec<i64> = (0..6 * t)
                .map(|_| i64::try_from(next(9)).unwrap() - 4)
                .collect();
            let is_ref: Vec<bool> = (0..3 * t).map(|_| next(3) == 0).collect();
            let refs: Vec<u32> = (0..t).map(|_| u32::try_from(next(6)).unwrap()).collect();
            let angles: Vec<i32> = (0..2 * t)
                .map(|_| i32::try_from(next(1024)).unwrap())
                .collect();
            let face_of = vec![0; t];
            let a = Arrays {
                tolerance: 0.5,
                origin: [0.0; 3],
                points: &pts,
                edge_status: &status,
                triangles: t,
                is_reference: &is_ref,
                references: &refs,
                ortho_turned: false,
                legacy_orient: next(2) == 0,
                normals: Some(NormalArrays {
                    bits: 10,
                    binary: &[false; 4],
                    angles: &angles,
                    planar: &[true],
                    face_of: &face_of,
                }),
            };
            let n = a.normals.as_ref().unwrap();
            let mut r = Resume::new(&a, n, 1, false);
            for flips in flip_sets(t) {
                let got = digest(r.attempt(&a, n, &flips));
                let want = digest(attempt(&a, n, &flips, false, false));
                kinds[usize::from(want.0)] += 1;
                assert!(got == want);
            }
        }
        assert!(kinds[1] > 0 && kinds[2] > 0, "{kinds:?}");
    }

    /// A tetrahedron closed by two references, its fourth normal record
    /// reversed: the stored normal orients the third triangle onto `[0 1]`,
    /// already shared twice, where no later choice can recover. Only the
    /// reading where the open edge overrides the normal fits.
    #[test]
    fn a_closed_edge_overrides_a_stored_normal_only_when_open_wins() {
        let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 0, 0, 4];
        let mut status = [0; 12];
        status[..3].copy_from_slice(&[2, 2, 2]);
        let mut is_ref = [false; 6];
        is_ref[4..].copy_from_slice(&[true, true]);
        let mut binary = [false; 16];
        binary[13] = true;
        let angles = [0; 8];
        let n = NormalArrays {
            bits: 10,
            binary: &binary,
            angles: &angles,
            planar: &[false],
            face_of: &[0; 4],
        };
        let a = Arrays {
            tolerance: 0.5,
            origin: [10.0, 0.0, 0.0],
            points: &pts,
            edge_status: &status,
            triangles: 4,
            is_reference: &is_ref,
            references: &[0, 2],
            ortho_turned: false,
            legacy_orient: false,
            normals: Some(n),
        };
        let tetra = [[0, 1, 2], [2, 1, 3], [3, 1, 0], [3, 0, 2]];
        assert_ne!(digest(attempt(&a, &n, &[], false, false)).0, 0);
        assert_eq!(digest(attempt(&a, &n, &[], false, true)).2, tetra);
        let plan = |open| Plan {
            depth: 0,
            open,
            first: true,
            ..Plan::unique(false, 0)
        };
        assert!(matches!(search(&a, &n, plan(false)), Search::NoFit));
        let fit = match search(&a, &n, plan(true)) {
            Search::Unique(m) => m.triangles,
            Search::Refused | Search::NoFit => Vec::new(),
        };
        assert_eq!(fit, tetra);
    }
    /// The same tetrahedron: the third triangle's fold comes from a stored
    /// normal, not a reversed bit, so only a loose walk offers it as a
    /// candidate, and flipping it fits. The records' normals leave their
    /// triangles' planes so none is blind.
    #[test]
    fn an_inexact_signalled_fold_is_a_candidate_only_when_loose() {
        let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 0, 0, 4];
        let mut status = [0; 12];
        status[..3].copy_from_slice(&[2, 2, 2]);
        let mut is_ref = [false; 6];
        is_ref[4..].copy_from_slice(&[true, true]);
        let mut binary = [false; 16];
        binary[13] = true;
        let angles = [0, 512, 0, 512, 0, 512, 0, 512];
        let n = NormalArrays {
            bits: 10,
            binary: &binary,
            angles: &angles,
            planar: &[false],
            face_of: &[0; 4],
        };
        let a = Arrays {
            tolerance: 0.5,
            origin: [10.0, 0.0, 0.0],
            points: &pts,
            edge_status: &status,
            triangles: 4,
            is_reference: &is_ref,
            references: &[0, 2],
            ortho_turned: false,
            legacy_orient: false,
            normals: Some(n),
        };
        let mut out = Vec::new();
        for loose in [false, true] {
            let mut r = Resume::new(&a, &n, 1, false);
            r.w.loose = loose;
            let c = match r.attempt(&a, &n, &[]) {
                Outcome::Failed(c) => c,
                Outcome::Fit(_) | Outcome::Dead => Vec::new(),
            };
            let fit = match r.attempt(&a, &n, &[Choice::Fold(2)]) {
                Outcome::Fit(m) => m.triangles,
                Outcome::Failed(_) | Outcome::Dead => Vec::new(),
            };
            out.push((c, fit));
        }
        let tetra = vec![[0, 1, 2], [2, 1, 3], [3, 1, 0], [3, 0, 2]];
        assert!(out[0].0.is_empty());
        assert!(out[1].0 == [Choice::Fold(2)]);
        assert_eq!(out[1].1, tetra);
    }
}

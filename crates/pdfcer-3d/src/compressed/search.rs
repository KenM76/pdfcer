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
/// How many candidate triangles before a failure are tried.
const WINDOW: usize = 40;
/// Triangle steps a search may spend before giving up undecided.
const STEP_BUDGET: usize = 2_000_000;
/// How many inverted choices the best-fit search combines.
const BEST_DEPTH: usize = 64;
/// Triangle steps the best-fit search spends on each reading.
const BEST_BUDGET: usize = 250_000;

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
#[derive(Clone, Copy, PartialEq, Eq)]
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
/// ([`Walk::open_wins`]). Measured on real producers, a normal can send
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

/// What one completed step of a resumed walk offers the search.
#[derive(Clone, Copy)]
struct Step {
    /// Whether the step began a component.
    start: bool,
    fold: bool,
    turn: bool,
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
            self.steps.push(Step {
                start,
                fold: !self.w.signalled || self.w.weak,
                turn: self.w.degenerate,
            });
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

    /// The last [`WINDOW`] candidates before a failure at `failed`, in its
    /// component and from `after` on, nearest last.
    fn candidates(&self, failed: usize, after: usize) -> Vec<Choice> {
        let mut c = Vec::new();
        for i in (after..failed).rev() {
            let Some(s) = self.steps.get(i) else { break };
            if s.turn {
                c.push(Choice::Turn(i));
            }
            if s.fold {
                c.push(Choice::Fold(i));
            }
            if c.len() >= WINDOW || s.start {
                break;
            }
        }
        c.truncate(WINDOW);
        c.reverse();
        c
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
    let mut candidates = Vec::new();
    for i in 0..a.triangles {
        if w.next.is_none() {
            candidates.clear();
        }
        let status = a.edge_status.get(i).copied().unwrap_or(0);
        w.turn = flips.contains(&Choice::Turn(i));
        if w.step(a, status, flips.contains(&Choice::Fold(i))).is_err() {
            return Outcome::Failed(candidates);
        }
        if (!w.signalled || w.weak) && i >= after {
            candidates.push(Choice::Fold(i));
        }
        if w.degenerate && i >= after {
            candidates.push(Choice::Turn(i));
        }
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

    /// A comparable digest of an outcome; a failure's candidates are cut
    /// to the window the search uses.
    fn digest(o: Outcome) -> (u8, Vec<Choice>, Vec<[u32; 3]>, Vec<V>) {
        match o {
            Outcome::Fit(m) => (0, Vec::new(), m.triangles, m.positions),
            Outcome::Failed(c) => {
                let skip = c.len().saturating_sub(WINDOW);
                (
                    1,
                    c.into_iter().skip(skip).collect(),
                    Vec::new(),
                    Vec::new(),
                )
            }
            Outcome::Dead => (2, Vec::new(), Vec::new(), Vec::new()),
        }
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
}

//! The planar-face check, and the last-resort search for a mesh no walk
//! fits.

use std::collections::HashMap;

use super::normals::NormalArrays;
use super::{
    Arrays, MAX_BEND, NormalReader, TriangleMesh, V, Walk, add, cross, dot, mesh, mul, sub, unit,
};

/// How many inverted choices one search combines.
const DEPTH: usize = 2;
/// How many candidate triangles before a failure are tried.
const WINDOW: usize = 40;
/// Triangle steps a search may spend before giving up undecided.
const STEP_BUDGET: usize = 8_000_000;

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

/// Searches combinations of at most [`DEPTH`] inverted choices for one
/// that rebuilds the mesh. A fold candidate is a triangle no stored
/// decision orients, or a sliver at most a tolerance from collinear, whose
/// decoded decision is noise; a turn candidate is an apex whose frame is
/// degenerate, whose turn rests on a rounding residue. A fit is one that
/// rebuilds the mesh: every array and stored normal consumed exactly and
/// every planar face flat. The rebuild is returned only if it is the
/// single such geometry the search finds within [`STEP_BUDGET`]; two, or a
/// search cut short, would make it a guess. Turns are tried only when no
/// fold-only combination fits at all.
pub(super) fn unique_fit(a: &Arrays<'_>) -> Option<TriangleMesh> {
    let n = a.normals.as_ref()?;
    match search(a, n, false) {
        Search::Unique(m) => Some(m),
        Search::Refused => None,
        Search::NoFit => match search(a, n, true) {
            Search::Unique(m) => Some(m),
            Search::Refused | Search::NoFit => None,
        },
    }
}

/// Whether a fold-only search rebuilds `a`.
#[cfg(test)]
pub(super) fn fits_without_turns(a: &Arrays<'_>) -> bool {
    a.normals
        .as_ref()
        .is_some_and(|n| matches!(search(a, n, false), Search::Unique(_)))
}

/// What one search came to.
enum Search {
    Unique(TriangleMesh),
    /// Two geometries fit, or the budget ran out after a fit.
    Refused,
    NoFit,
}

/// One depth-first search; turn candidates are offered only with `turns`,
/// so a fold-only rebuild is never second-guessed by a turned one.
fn search(a: &Arrays<'_>, n: &NormalArrays<'_>, turns: bool) -> Search {
    let walks = STEP_BUDGET / a.triangles.max(1);
    let mut stack = vec![Vec::new()];
    let mut found: Option<TriangleMesh> = None;
    for _ in 0..walks {
        let Some(flips) = stack.pop() else {
            return found.map_or(Search::NoFit, Search::Unique);
        };
        match attempt(a, n, &flips) {
            Outcome::Fit(m) => match &found {
                Some(f) if f.triangles != m.triangles || f.positions != m.positions => {
                    return Search::Refused;
                }
                _ => found = Some(m),
            },
            Outcome::Failed(c) if flips.len() < DEPTH => {
                let skip = c.len().saturating_sub(WINDOW);
                for j in c.into_iter().skip(skip) {
                    if turns || matches!(j, Choice::Fold(_)) {
                        let mut g = flips.clone();
                        g.push(j);
                        stack.push(g);
                    }
                }
            }
            Outcome::Failed(_) | Outcome::Dead => {}
        }
    }
    if found.is_some() {
        Search::Refused
    } else {
        Search::NoFit
    }
}

/// Walks the whole mesh oriented, inverting each of `flips`, with no
/// retries.
fn attempt(a: &Arrays<'_>, n: &NormalArrays<'_>, flips: &[Choice]) -> Outcome {
    let mut w = Walk::new(a.triangles, Some(NormalReader::new(n)));
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

//! The planar-face check, and the last-resort search for a mesh no walk
//! fits.

use std::collections::HashMap;

use super::normals::NormalArrays;
use super::{
    Arrays, MAX_BEND, NormalReader, TriangleMesh, V, Walk, add, cross, dot, mesh, mul, sub, unit,
};

/// How many inverted folds one search combines.
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

/// What one walk with a fixed set of inverted folds came to.
enum Outcome {
    Fit(TriangleMesh),
    /// Failed at a triangle; the candidates before it, in its
    /// component and after the last inversion, nearest last.
    Failed(Vec<usize>),
    /// Failed in a way no further inversion can repair.
    Dead,
}

/// Searches combinations of at most [`DEPTH`] inverted folds for one that
/// rebuilds the mesh. A candidate is a triangle no stored decision
/// orients, or a sliver at most a tolerance from collinear, whose decoded
/// decision is noise. A fit is one that rebuilds the mesh: every array and stored normal consumed
/// exactly and every planar face flat. The rebuild is returned only if it
/// is the single such fit the search finds within [`STEP_BUDGET`]; two
/// fits, or a search cut short, would make it a guess.
pub(super) fn unique_fit(a: &Arrays<'_>) -> Option<TriangleMesh> {
    let n = a.normals.as_ref()?;
    let walks = STEP_BUDGET / a.triangles.max(1);
    let mut stack = vec![Vec::new()];
    let mut found: Option<TriangleMesh> = None;
    for _ in 0..walks {
        let Some(flips) = stack.pop() else {
            return found;
        };
        match attempt(a, n, &flips) {
            Outcome::Fit(m) => match &found {
                Some(f) if f.triangles != m.triangles => return None,
                _ => found = Some(m),
            },
            Outcome::Failed(c) if flips.len() < DEPTH => {
                let skip = c.len().saturating_sub(WINDOW);
                for j in c.into_iter().skip(skip) {
                    let mut g = flips.clone();
                    g.push(j);
                    stack.push(g);
                }
            }
            Outcome::Failed(_) | Outcome::Dead => {}
        }
    }
    None
}

/// Walks the whole mesh oriented, inverting the fold at each of `flips`
/// (ascending), with no retries.
fn attempt(a: &Arrays<'_>, n: &NormalArrays<'_>, flips: &[usize]) -> Outcome {
    let mut w = Walk::new(a.triangles, Some(NormalReader::new(n)));
    let after = flips.last().map_or(0, |f| f + 1);
    let mut candidates = Vec::new();
    for i in 0..a.triangles {
        if w.next.is_none() {
            candidates.clear();
        }
        let status = a.edge_status.get(i).copied().unwrap_or(0);
        if w.step(a, status, flips.contains(&i)).is_err() {
            return Outcome::Failed(candidates);
        }
        if (!w.signalled || w.weak) && i >= after {
            candidates.push(i);
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

//! `TESS_3D_Wire` index words turned into polylines [WD 7.8.7.5], and the
//! damage containment a lenient read applies to them.

use crate::PrcError;
use crate::prc::tess::face::point_at;
use crate::prc::tess::malformed;

/// A wire header's count bits; the top four are flags [WD 7.8.7.5].
const WIRE_COUNT_MASK: u32 = 0x0FFF_FFFF;
pub(crate) const WIRE_IS_CLOSING: u32 = 0x1000_0000;
pub(crate) const WIRE_IS_CONTINUOUS: u32 = 0x2000_0000;

/// Wire polylines, each a run of points.
pub(crate) type Polylines = Vec<Vec<[f64; 3]>>;

/// `flat` cut into runs as long as each of `wires`, or nothing when the
/// lengths disagree.
pub(crate) fn split_like(wires: &[Vec<[f64; 3]>], flat: Vec<[u8; 4]>) -> Vec<Vec<[u8; 4]>> {
    if flat.len() != wires.iter().map(Vec::len).sum::<usize>() {
        return Vec::new();
    }
    let mut rest = flat.into_iter();
    wires
        .iter()
        .map(|w| rest.by_ref().take(w.len()).collect())
        .collect()
}

/// The polylines `words` build over `pts`, each with a flag that it names
/// the partial point after `pts` (when `partial`); a flagged polyline
/// holds a placeholder there, so its length still matches its colours.
pub(crate) fn wire_polylines(
    pts: &[[f64; 3]],
    partial: bool,
    words: &[u32],
) -> Result<(Polylines, Vec<bool>), PrcError> {
    let mut wires: Vec<Vec<[f64; 3]>> = Vec::new();
    let mut bad = Vec::new();
    if words.is_empty() {
        if !pts.is_empty() {
            wires.push(pts.to_vec());
            bad.push(false);
        }
        return Ok((wires, bad));
    }
    let names_partial =
        |idx: u32| partial && idx.is_multiple_of(3) && idx as usize / 3 == pts.len();
    let mut it = words.iter().copied();
    while let Some(header) = it.next() {
        let n = (header & WIRE_COUNT_MASK) as usize;
        let mut wire = Vec::with_capacity(n.min(words.len()));
        let mut broken = false;
        for _ in 0..n {
            let idx = it
                .next()
                .ok_or_else(|| malformed("wire runs past its indices".into()))?;
            if names_partial(idx) {
                broken = true;
                wire.push([0.0; 3]);
            } else {
                wire.push(point_at(pts, idx)?);
            }
        }
        if header & WIRE_IS_CLOSING != 0
            && let Some(&first) = wire.first()
        {
            wire.push(first);
        }
        match (wires.last_mut(), bad.last_mut()) {
            (Some(prev), Some(b)) if header & WIRE_IS_CONTINUOUS != 0 => {
                prev.extend(wire);
                *b |= broken;
            }
            _ => {
                wires.push(wire);
                bad.push(broken);
            }
        }
    }
    Ok((wires, bad))
}

/// The colour count `words` call for: each header's points plus one for a
/// closing wire, or `points` when there are no words [WD 7.8.7.2].
pub(crate) fn wire_colour_count(words: &[u32], points: usize) -> usize {
    if words.is_empty() {
        return points;
    }
    let mut colours = 0usize;
    let mut it = words.iter().copied();
    while let Some(header) = it.next() {
        let n = (header & WIRE_COUNT_MASK) as usize;
        colours = colours.saturating_add(n);
        if header & WIRE_IS_CLOSING != 0 {
            colours = colours.saturating_add(1);
        }
        if n > 0 && it.nth(n - 1).is_none() {
            break;
        }
    }
    colours
}

/// `v` without the entries `bad` flags.
pub(crate) fn unflagged<T>(v: Vec<T>, bad: &[bool]) -> Vec<T> {
    v.into_iter()
        .zip(bad.iter().chain(std::iter::repeat(&false)))
        .filter(|(_, b)| !**b)
        .map(|(x, _)| x)
        .collect()
}

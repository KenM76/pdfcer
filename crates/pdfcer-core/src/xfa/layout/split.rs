//! Splitting a flowed instance across content areas (XFA 3.3 Ch.8
//! pp.290-295).

use super::LayoutRect;
use super::geometry::{ItemFlags, Placed};

/// Tolerance for an edge that lies on a split line.
const EPS: f64 = 1e-6;

/// The two parts of a split, with the index in the split instance's items
/// of every item of each: an item cut in two appears in both.
pub(super) struct Cut {
    pub(super) head: Placed,
    pub(super) tail: Placed,
    pub(super) head_src: Vec<usize>,
    pub(super) tail_src: Vec<usize>,
}

impl Cut {
    /// Indexes in `head` and `tail` of each item cut in two, outermost
    /// (the instance itself) first.
    pub(super) fn crossing(&self) -> Vec<(usize, usize)> {
        let (mut out, mut t) = (Vec::new(), 0);
        for (h, src) in self.head_src.iter().enumerate() {
            while self.tail_src.get(t).is_some_and(|s| s < src) {
                t += 1;
            }
            if self.tail_src.get(t) == Some(src) {
                out.push((h, t));
            }
        }
        out
    }
}

/// Split `p` at the highest line at or above `room` that every unbreakable
/// item and every adhering pair accepts ("creep upward", p.293). The first
/// part keeps the items above the line; a breakable item crossing it is cut
/// in two. A line falling between two adhering siblings moves into the
/// earlier one: it is cut when breakable and moves with its partner when
/// not (pp.311-314). `Err(p)` when `p` is unbreakable or no line above
/// `room` leaves content in the first part and content in the second
/// (never a split inside a margin, p.290).
pub(super) fn split(p: Placed, room: f64) -> Result<Cut, Placed> {
    if !p.flags.first().is_some_and(|f| f.breakable) || room <= EPS {
        return Err(p);
    }
    let mut s = room;
    while let Some(y) = blocker(&p, s) {
        s = y;
    }
    let above = p
        .items
        .iter()
        .skip(1)
        .any(|i| i.rect.y + i.rect.h <= s + EPS);
    let below = p
        .items
        .iter()
        .skip(1)
        .any(|i| i.rect.y + i.rect.h > s + EPS);
    if s <= EPS || !above || !below {
        return Err(p);
    }
    let (mut first, mut rest) = (empty(&p, s), empty(&p, p.rect.h - s));
    let (mut head_src, mut tail_src) = (Vec::new(), Vec::new());
    for (k, (mut item, b)) in p.items.into_iter().zip(p.flags).enumerate() {
        let r = item.rect;
        if r.y + r.h <= s + EPS {
            first.push(item, b);
            head_src.push(k);
        } else if r.y >= s - EPS {
            item.rect.y -= s;
            rest.push(item, b);
            tail_src.push(k);
        } else {
            let mut tail = item.clone();
            item.rect.h = s - r.y;
            tail.rect.y = 0.0;
            tail.rect.h = r.y + r.h - s;
            first.push(item, b);
            rest.push(tail, b);
            head_src.push(k);
            tail_src.push(k);
        }
    }
    reglue(&mut first, &head_src);
    reglue(&mut rest, &tail_src);
    Ok(Cut {
        head: first,
        tail: rest,
        head_src,
        tail_src,
    })
}

/// The highest line above `s` that the item or adhering pair `s` would
/// break must move to, if any.
fn blocker(p: &Placed, s: f64) -> Option<f64> {
    let crosses = |r: &LayoutRect| r.y + EPS < s && s < r.y + r.h - EPS;
    let cut = p
        .items
        .iter()
        .zip(&p.flags)
        .skip(1)
        .filter(|(i, f)| !f.breakable && crosses(&i.rect))
        .map(|(i, _)| i.rect.y);
    let seams = p
        .items
        .iter()
        .zip(&p.flags)
        .enumerate()
        .filter_map(|(j, (b, f))| {
            let k = j.checked_sub(f.glue).filter(|_| f.glue > 0)?;
            let (a, af) = (p.items.get(k)?, p.flags.get(k)?);
            let bottom = a.rect.y + a.rect.h;
            (bottom <= s + EPS && b.rect.y >= s - EPS && a.rect.h > EPS).then_some(
                if af.breakable {
                    bottom - 2.0 * EPS
                } else {
                    a.rect.y
                },
            )
        });
    cut.chain(seams).reduce(f64::min)
}

/// Keep each glue only where both partners landed in the same part;
/// `src`: each item's index in the unsplit instance.
fn reglue(part: &mut Placed, src: &[usize]) {
    for j in 0..part.flags.len() {
        let Some(f) = part.flags.get_mut(j) else {
            continue;
        };
        if f.glue == 0 {
            continue;
        }
        let want = src.get(j).and_then(|s| s.checked_sub(f.glue));
        f.glue = want
            .and_then(|w| src.iter().position(|&x| x == w))
            .map_or(0, |k| j - k);
    }
}

fn empty(p: &Placed, h: f64) -> Placed {
    Placed {
        rect: LayoutRect { h, ..p.rect },
        items: Vec::new(),
        flags: Vec::new(),
        keep: p.keep,
        path: p.path.clone(),
    }
}

impl Placed {
    /// Append `item` with what a split line must respect at it.
    pub(super) fn push(&mut self, item: super::LayoutItem, flags: ItemFlags) {
        self.items.push(item);
        self.flags.push(flags);
    }
}

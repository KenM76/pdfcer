//! Splitting a flowed instance across content areas (XFA 3.3 Ch.8
//! pp.290-295).

use super::LayoutRect;
use super::geometry::Placed;

/// Tolerance for an edge that lies on a split line.
const EPS: f64 = 1e-6;

/// Split `p` at the highest line at or above `room` that every unbreakable
/// item accepts ("creep upward", p.293). The first part keeps the items
/// above the line; a breakable item crossing it is cut in two. `Err(p)` when
/// `p` is unbreakable or no line above `room` leaves content in the first
/// part and content in the second (never a split inside a margin, p.290).
pub(super) fn split(p: Placed, room: f64) -> Result<(Placed, Placed), Placed> {
    if !p.breakable.first().copied().unwrap_or(false) || room <= EPS {
        return Err(p);
    }
    let crosses = |r: &LayoutRect, s: f64| r.y + EPS < s && s < r.y + r.h - EPS;
    let mut s = room;
    loop {
        let blocker = p
            .items
            .iter()
            .zip(&p.breakable)
            .skip(1)
            .filter(|(i, b)| !**b && crosses(&i.rect, s))
            .map(|(i, _)| i.rect.y)
            .reduce(f64::min);
        match blocker {
            Some(y) => s = y,
            None => break,
        }
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
    for (mut item, b) in p.items.into_iter().zip(p.breakable) {
        let r = item.rect;
        if r.y + r.h <= s + EPS {
            first.push(item, b);
        } else if r.y >= s - EPS {
            item.rect.y -= s;
            rest.push(item, b);
        } else {
            let mut tail = item.clone();
            item.rect.h = s - r.y;
            tail.rect.y = 0.0;
            tail.rect.h = r.y + r.h - s;
            first.push(item, b);
            rest.push(tail, b);
        }
    }
    Ok((first, rest))
}

fn empty(p: &Placed, h: f64) -> Placed {
    Placed {
        rect: LayoutRect { h, ..p.rect },
        items: Vec::new(),
        breakable: Vec::new(),
        keep: p.keep,
    }
}

impl Placed {
    fn push(&mut self, item: super::LayoutItem, breakable: bool) {
        self.items.push(item);
        self.breakable.push(breakable);
    }
}

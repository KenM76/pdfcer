//! Overflow inside a split instance: a nested subform the split line cuts
//! overflows with it (XFA 3.3 Ch.8 pp.318-325; gap G-L21, setting
//! [`super::NestedOverflow`]).

use std::collections::HashMap;

use super::NestedOverflow;
use super::geometry::{Placed, shifted};
use super::overflow::{Link, Overflow};
use super::split::{Cut, split};
use crate::xfa::ContainerKind;

/// Reservation passes before a split is taken as it falls.
const RESERVE_TRIES: usize = 4;

/// The instance being put, across its splits: each current item's index in
/// the unsplit instance, and the level of each nested subform instance met
/// so far, keyed on that index (use counts are per instance, p.323).
pub(super) struct Nesting {
    ids: Vec<usize>,
    levels: HashMap<usize, Link>,
}

impl Nesting {
    pub(super) fn new(p: &Placed) -> Self {
        Self {
            ids: (0..p.items.len()).collect(),
            levels: HashMap::new(),
        }
    }
}

/// One subform laying a leader and trailer at a split: `at` its head and
/// tail item indexes (`None`: the instance itself, laid outside it), and
/// the chain it spends per kind (`None`: it lays none of that kind).
struct Overflower {
    at: Option<(usize, usize)>,
    leaders: Option<Vec<Link>>,
    trailers: Option<Vec<Link>>,
}

/// A cut subform: head and tail item indexes, template path, and the
/// unsplit instance's index of its item.
struct Sub {
    at: (usize, usize),
    path: Vec<usize>,
    id: usize,
}

/// A split with its nested leaders and trailers in place.
pub(super) struct Split {
    pub(super) head: Placed,
    pub(super) tail: Placed,
    /// The instance's own trailer, laid after `head`.
    pub(super) trailer: Vec<Placed>,
    /// Does the instance lay its own leader before `tail`?
    pub(super) lead: bool,
}

impl super::Run<'_, '_> {
    /// Split `p` within `room`, reserving the trailers the split will lay
    /// (p.320): each subform that overflows lays its trailer at the end of
    /// its own top fragment, innermost first, and its leader at the start
    /// of its own rest, outermost first.
    pub(super) fn split_overflowing(
        &mut self,
        p: Placed,
        ov: &mut Overflow,
        nest: &mut Nesting,
        room: f64,
    ) -> Result<Split, Placed> {
        let mut reserve = self.trailer_height(ov, ov.chain(false));
        let mut tries = 0;
        let (mut cut, who) = loop {
            let Ok(cut) = split(p.clone(), room - reserve) else {
                return Err(p);
            };
            let who = self.overflowers(ov, nest, &cut);
            let need: f64 = who
                .iter()
                .filter_map(|w| w.trailers.as_deref())
                .map(|c| self.trailer_height(ov, c))
                .sum();
            tries += 1;
            if need <= reserve + 1e-6 || tries == RESERVE_TRIES {
                break (cut, who);
            }
            reserve = need;
        };
        let (mut trailer, mut lead) = (Vec::new(), false);
        let mut trailers = Vec::new();
        for w in &who {
            let Some(c) = &w.trailers else { continue };
            let parts = self.take_in(ov, c, false);
            match w.at {
                Some((h, _)) => trailers.push((h, parts)),
                None => trailer = parts,
            }
        }
        let mut leaders = Vec::new();
        for w in who.iter().rev() {
            match (w.at, &w.leaders) {
                (None, Some(_)) => lead = true,
                (Some((_, t)), Some(c)) => leaders.push((t, self.take_in(ov, c, true))),
                _ => {}
            }
        }
        let crossing = cut.crossing();
        let ids: Vec<usize> = cut
            .tail_src
            .iter()
            .map(|&k| nest.ids.get(k).copied().unwrap_or(usize::MAX))
            .collect();
        let heads: Vec<usize> = crossing.iter().map(|c| c.0).collect();
        let tails: Vec<usize> = crossing.iter().map(|c| c.1).collect();
        add_trailers(&mut cut.head, &heads, trailers);
        add_leaders(&mut cut.tail, &tails, leaders);
        nest.ids = ids;
        nest.ids.resize(cut.tail.items.len(), usize::MAX);
        Ok(Split {
            head: cut.head,
            tail: cut.tail,
            trailer,
            lead,
        })
    }

    /// The nested subforms `cut` cuts, outermost first.
    fn subs(nest: &Nesting, cut: &Cut) -> Vec<Sub> {
        cut.crossing()
            .into_iter()
            .filter(|&(h, _)| h != 0)
            .filter_map(|(h, t)| {
                let item = cut.head.items.get(h)?;
                (item.kind == ContainerKind::Subform).then(|| Sub {
                    at: (h, t),
                    path: item.path.clone(),
                    id: cut
                        .head_src
                        .get(h)
                        .and_then(|&k| nest.ids.get(k))
                        .copied()
                        .unwrap_or(usize::MAX),
                })
            })
            .collect()
    }

    /// Who overflows at `cut`, innermost first. Without a nested subform
    /// cut, the instance alone, spending its chain. Otherwise the innermost
    /// cut subform spends its own chain up through the instance's; under
    /// [`NestedOverflow::EachAsserting`] every enclosing cut subform, the
    /// instance included, that asserts a kind of its own also lays it.
    fn overflowers(&mut self, ov: &Overflow, nest: &mut Nesting, cut: &Cut) -> Vec<Overflower> {
        let subs = Self::subs(nest, cut);
        if subs.is_empty() {
            return vec![Overflower {
                at: None,
                leaders: Some(ov.chain(true).to_vec()),
                trailers: Some(ov.chain(false).to_vec()),
            }];
        }
        let top_len = cut.head.items.first().map_or(0, |i| i.path.len());
        // Innermost first: each cut subform's own level, then the levels
        // between it and the next cut subform out.
        let mut starts: Vec<(Option<(usize, usize)>, usize)> = Vec::new();
        let mut chain = Vec::new();
        for (i, sub) in subs.iter().enumerate().rev() {
            let own = match nest.levels.get(&sub.id) {
                Some(&l) => l,
                None => {
                    let l = self.nested_level(&sub.path);
                    nest.levels.insert(sub.id, l);
                    l
                }
            };
            starts.push((Some(sub.at), chain.len()));
            chain.push(own);
            let outer = i.checked_sub(1).and_then(|o| subs.get(o));
            let stop = outer.map_or(top_len, |o| o.path.len()) + 1;
            chain.extend(self.levels_between(&sub.path, stop));
        }
        starts.push((None, chain.len()));
        chain.push(Link::Own);
        chain.extend(ov.ups.iter().copied());
        let each = self.ctx.opts.nested_overflow == NestedOverflow::EachAsserting;
        let mut who = Vec::new();
        for (k, &(at, from)) in starts.iter().enumerate() {
            let rest = chain.get(from..).unwrap_or_default();
            let pick = |leader| {
                let first = rest.first().copied();
                let own = first.is_some_and(|l| self.asserts(ov, l, leader));
                (k == 0 || (each && own)).then(|| self.chain_for(ov, rest, leader))
            };
            let (leaders, trailers) = (pick(true), pick(false));
            if leaders.is_some() || trailers.is_some() {
                who.push(Overflower {
                    at,
                    leaders,
                    trailers,
                });
            }
        }
        who
    }
}

/// Append `parts` stacked from `y` at `x`; the `y` below them.
fn stack(p: &mut Placed, parts: Vec<Placed>, x: f64, mut y: f64) -> f64 {
    for part in parts {
        let h = part.rect.h;
        for item in shifted(part.items, x, y) {
            p.push(item, false);
        }
        y += h;
    }
    y
}

/// Lay each `(item, parts)` trailer, innermost first, below the split line
/// at its subform's left edge, growing every cut container around it.
fn add_trailers(head: &mut Placed, cut: &[usize], laid: Vec<(usize, Vec<Placed>)>) {
    let mut y = head.rect.h;
    let mut ends: Vec<(Vec<usize>, f64)> = Vec::new();
    for (h, parts) in laid {
        let Some((x, path)) = head.items.get(h).map(|i| (i.rect.x, i.path.clone())) else {
            continue;
        };
        y = stack(head, parts, x, y);
        ends.push((path, y));
    }
    for &k in cut {
        let Some(item) = head.items.get_mut(k) else {
            continue;
        };
        let end = ends
            .iter()
            .filter(|(p, _)| p.starts_with(&item.path))
            .map(|e| e.1)
            .fold(item.rect.y + item.rect.h, f64::max);
        item.rect.h = end - item.rect.y;
    }
    head.rect.h = y;
}

/// Lay each `(item, parts)` leader, outermost first, at the top of its
/// subform's rest, pushing everything below it down.
fn add_leaders(tail: &mut Placed, cut: &[usize], laid: Vec<(usize, Vec<Placed>)>) {
    let heights: Vec<(Vec<usize>, f64)> = laid
        .iter()
        .filter_map(|(t, parts)| {
            let path = tail.items.get(*t)?.path.clone();
            Some((path, parts.iter().map(|p| p.rect.h).sum()))
        })
        .collect();
    let total: f64 = heights.iter().map(|h| h.1).sum();
    if total <= 0.0 {
        return;
    }
    for (j, item) in tail.items.iter_mut().enumerate() {
        if !cut.contains(&j) {
            item.rect.y += total;
            continue;
        }
        let inside: f64 = heights
            .iter()
            .filter(|(p, _)| p.starts_with(&item.path))
            .map(|h| h.1)
            .sum();
        item.rect.y += total - inside;
        item.rect.h += inside;
    }
    let mut y = 0.0;
    for (t, parts) in laid {
        let x = tail.items.get(t).map_or(0.0, |i| i.rect.x);
        y = stack(tail, parts, x, y);
    }
    tail.rect.h += total;
}

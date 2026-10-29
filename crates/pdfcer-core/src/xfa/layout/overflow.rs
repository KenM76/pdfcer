//! Overflow leaders and trailers, `overflow@leader` / `overflow@trailer`
//! (XFA 3.3 Ch.8 pp.318-325, Ch.17 p.791).

use std::collections::HashMap;

use super::LayoutWarning;
use super::geometry::{Ctx, Placed, instances};
use super::resolve;
use crate::xfa::{ContainerKind, LayoutStrategy, XfaNode};

/// One subform's `overflow` assertion, per kind: `None` when it does not
/// assert that kind (transparent: inherited from its container), `Some` of
/// an empty list when it blocks inheritance (`leader=""`, p.325, gap
/// G-L22). Use counts start at zero per asserting subform (pp.323-324); a
/// subform named as both a leader and a trailer spends one count (p.322).
pub(super) struct Level {
    leaders: Option<Vec<Target>>,
    trailers: Option<Vec<Target>>,
    used: HashMap<Vec<usize>, u32>,
}

struct Target {
    path: Vec<usize>,
    /// Most uses; a leader with no `occur` element is unlimited (gap G-L17).
    max: u32,
    /// Its instances, laid once, stacked top to bottom.
    parts: Vec<Placed>,
}

impl Level {
    fn none() -> Self {
        Self {
            leaders: None,
            trailers: None,
            used: HashMap::new(),
        }
    }

    pub(super) fn of(ctx: &mut Ctx<'_>, root: &XfaNode, node: &XfaNode, path: &[usize]) -> Self {
        let o = node.child("overflow");
        let mut list = |a| {
            o.and_then(|o| o.attr(a))
                .map(|v| targets(ctx, root, v, path))
        };
        Self {
            leaders: list("leader"),
            trailers: list("trailer"),
            used: HashMap::new(),
        }
    }

    /// Does this level assert a non-empty list of that kind?
    pub(super) fn asserts(&self, leader: bool) -> bool {
        self.list(leader).is_some_and(|l| !l.is_empty())
    }

    fn list(&self, leader: bool) -> Option<&[Target]> {
        if leader {
            self.leaders.as_deref()
        } else {
            self.trailers.as_deref()
        }
    }

    fn next(&self, leader: bool) -> Option<&Target> {
        self.list(leader)?
            .iter()
            .find(|t| self.used.get(&t.path).copied().unwrap_or(0) < t.max)
    }
}

/// The leaders and trailers one instance draws on, per kind: its own level,
/// then each containing `subform`/`subformSet` it inherits from, nearest
/// first, up to one that blocks. Once a level's targets are spent the next
/// level's are (p.325); a containing level's counts are shared by every
/// instance inheriting them (p.323, gap G-L23).
pub(super) struct Overflow {
    pub(super) own: Level,
    /// The containing levels above the instance, nearest first.
    pub(super) ups: Vec<Link>,
    leaders: Vec<Link>,
    trailers: Vec<Link>,
}

/// One level of a chain: the instance's own, or one in `Run::levels`.
#[derive(Clone, Copy)]
pub(super) enum Link {
    Own,
    Shared(usize),
}

impl Overflow {
    pub(super) fn chain(&self, leader: bool) -> &[Link] {
        if leader {
            &self.leaders
        } else {
            &self.trailers
        }
    }
}

/// Resolve a leader/trailer list: items separated by one U+0020, `#id` an
/// XML ID, otherwise a name (p.322). Only a bare name is resolved as a SOM
/// expression yet. An item naming no single `subform`/`subformSet` is
/// warned and skipped (p.323).
fn targets(ctx: &mut Ctx<'_>, root: &XfaNode, list: &str, path: &[usize]) -> Vec<Target> {
    let mut out = Vec::new();
    for spec in list.split(' ').filter(|s| !s.is_empty()) {
        let Some((node, at)) = find(root, spec) else {
            ctx.warn(LayoutWarning::InvalidOverflowTarget {
                path: path.to_vec(),
                target: spec.to_owned(),
            });
            continue;
        };
        let max = match node.child("occur") {
            None => u32::MAX,
            Some(_) => node.occur().and_then(|o| o.max).unwrap_or(u32::MAX),
        };
        let mut parts = Vec::new();
        instances(ctx, node, &at, LayoutStrategy::Tb, &mut parts);
        out.push(Target {
            path: at,
            max,
            parts,
        });
    }
    out
}

/// The one `subform`/`subformSet` under `root` whose `id` (for `#id`) or
/// `name` is `spec`, with its path from `root`.
pub(super) fn find<'t>(root: &'t XfaNode, spec: &str) -> Option<(&'t XfaNode, Vec<usize>)> {
    let (attr, want) = spec
        .strip_prefix('#')
        .map_or(("name", spec), |id| ("id", id));
    let mut found = Vec::new();
    let mut stack = vec![(root, Vec::new())];
    while let Some((n, p)) = stack.pop() {
        for (i, c) in n.children.iter().enumerate() {
            let mut cp = p.clone();
            cp.push(i);
            if matches!(c.tag.as_str(), "subform" | "subformSet") && c.attr(attr) == Some(want) {
                found.push((c, cp.clone()));
            }
            stack.push((c, cp));
        }
    }
    if found.len() == 1 { found.pop() } else { None }
}

impl super::Run<'_, '_> {
    /// The overflow chain of the top-level instance at `path`, laid by a
    /// `tb` root (the only one that splits).
    pub(super) fn overflow_of(&mut self, path: &[usize]) -> Overflow {
        let root = self.root;
        let own = match resolve(root, &[], path) {
            Some(n) => Level::of(&mut self.ctx, root, n, path),
            None => Level::none(),
        };
        let ups = self.levels_between(path, 1);
        let mut ov = Overflow {
            own,
            ups,
            leaders: Vec::new(),
            trailers: Vec::new(),
        };
        let links: Vec<Link> = std::iter::once(Link::Own)
            .chain(ov.ups.iter().copied())
            .collect();
        ov.leaders = self.chain_for(&ov, &links, true);
        ov.trailers = self.chain_for(&ov, &links, false);
        ov
    }

    /// The shared levels of the `subform`/`subformSet` ancestors of
    /// `path` whose paths are at least `stop` long, nearest first.
    pub(super) fn levels_between(&mut self, path: &[usize], stop: usize) -> Vec<Link> {
        let root = self.root;
        let mut ups = Vec::new();
        for n in (stop..path.len()).rev() {
            let (Some(at), Some(up)) = (
                path.get(..n),
                path.get(..n).and_then(|a| resolve(root, &[], a)),
            ) else {
                continue;
            };
            if matches!(
                up.container_kind(),
                Some(ContainerKind::Subform | ContainerKind::SubformSet)
            ) {
                ups.push(Link::Shared(self.shared_level(up, at)));
            }
        }
        ups
    }

    /// `links` up to and including the first that blocks inheritance of
    /// that kind, skipping those that do not assert it (p.325).
    pub(super) fn chain_for(&self, ov: &Overflow, links: &[Link], leader: bool) -> Vec<Link> {
        let mut chain = Vec::new();
        for &link in links {
            let Some(list) = self.level(ov, link).and_then(|l| l.list(leader)) else {
                continue;
            };
            chain.push(link);
            if list.is_empty() {
                break;
            }
        }
        chain
    }

    /// A new level for the nested instance at `path`, in `Run::levels`.
    pub(super) fn nested_level(&mut self, path: &[usize]) -> Link {
        let root = self.root;
        let level = match resolve(root, &[], path) {
            Some(n) => Level::of(&mut self.ctx, root, n, path),
            None => Level::none(),
        };
        self.levels.push(level);
        Link::Shared(self.levels.len() - 1)
    }

    pub(super) fn asserts(&self, ov: &Overflow, link: Link, leader: bool) -> bool {
        self.level(ov, link).is_some_and(|l| l.asserts(leader))
    }

    fn shared_level(&mut self, node: &XfaNode, at: &[usize]) -> usize {
        if let Some(&i) = self.level_at.get(at) {
            return i;
        }
        let level = Level::of(&mut self.ctx, self.root, node, at);
        self.levels.push(level);
        let i = self.levels.len() - 1;
        self.level_at.insert(at.to_vec(), i);
        i
    }

    fn level<'a>(&'a self, ov: &'a Overflow, link: Link) -> Option<&'a Level> {
        match link {
            Link::Own => Some(&ov.own),
            Link::Shared(i) => self.levels.get(i),
        }
    }

    fn next<'a>(
        &'a self,
        ov: &'a Overflow,
        chain: &[Link],
        leader: bool,
    ) -> Option<(Link, &'a Target)> {
        chain.iter().find_map(|&link| {
            self.level(ov, link)
                .and_then(|l| l.next(leader))
                .map(|t| (link, t))
        })
    }

    /// Height of the trailer the next overflow would lay: the space to
    /// reserve now (p.320, gap G-L21).
    pub(super) fn trailer_height(&self, ov: &Overflow, chain: &[Link]) -> f64 {
        self.next(ov, chain, false)
            .map_or(0.0, |(_, t)| t.parts.iter().map(|p| p.rect.h).sum())
    }

    /// Spend one use of the instance's next leader (or trailer).
    pub(super) fn take_overflow(&mut self, ov: &mut Overflow, leader: bool) -> Vec<Placed> {
        let chain = ov.chain(leader).to_vec();
        self.take_in(ov, &chain, leader)
    }

    /// Spend one use of the next leader (or trailer) along `chain` and
    /// return its parts. Each copy spends its items against
    /// [`super::MAX_LAYOUT_ITEMS`].
    pub(super) fn take_in(
        &mut self,
        ov: &mut Overflow,
        chain: &[Link],
        leader: bool,
    ) -> Vec<Placed> {
        let Some((link, path, parts)) = self
            .next(ov, chain, leader)
            .map(|(l, t)| (l, t.path.clone(), t.parts.clone()))
        else {
            return Vec::new();
        };
        let cost: usize = parts.iter().map(|p| p.items.len()).sum();
        if self.ctx.items.saturating_add(cost) > super::MAX_LAYOUT_ITEMS {
            self.ctx.warn(LayoutWarning::ItemLimit);
            return Vec::new();
        }
        self.ctx.items += cost;
        let level = match link {
            Link::Own => Some(&mut ov.own),
            Link::Shared(i) => self.levels.get_mut(i),
        };
        if let Some(l) = level {
            *l.used.entry(path).or_insert(0) += 1;
        }
        parts
    }

    /// Move on after the instance at `at` splits: to its own
    /// `overflow@target`, a `contentArea` (looked for first) or `pageArea`
    /// of the root `pageSet`, else to the next content area (Ch.17 p.791,
    /// Ch.9 p.366). The target is not inherited; only leaders and trailers
    /// are (p.323). One naming no single area warns and moves on normally.
    pub(super) fn overflow_move(&mut self, at: &[usize]) {
        let target = resolve(self.root, &[], at)
            .and_then(|n| n.child("overflow"))
            .and_then(|o| o.attr("target"))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned);
        let Some(t) = target else {
            self.area_index += 1;
            return;
        };
        if ["contentArea", "pageArea"]
            .iter()
            .any(|kind| self.break_to_target(&t, kind, false))
        {
            return;
        }
        self.ctx.warn(LayoutWarning::InvalidOverflowTarget {
            path: at.to_vec(),
            target: t,
        });
        self.area_index += 1;
    }

    /// Lay `parts` one after another in the current area's flow.
    pub(super) fn put_parts(&mut self, parts: Vec<Placed>, ca: super::LayoutRect) {
        for p in parts {
            let at = self.flow.slot(p.rect.w);
            self.flow.commit(p.rect.w, p.rect.h);
            self.emit(p, ca, at);
        }
    }
}

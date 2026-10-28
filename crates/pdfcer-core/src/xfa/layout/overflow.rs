//! Overflow leaders and trailers, `overflow@leader` / `overflow@trailer`
//! (XFA 3.3 Ch.8 pp.318-325, Ch.17 p.791).

use std::collections::HashMap;

use super::LayoutWarning;
use super::geometry::{Ctx, Placed, instances};
use crate::xfa::{LayoutStrategy, XfaNode};

/// The leaders and trailers one overflowing instance lays. Its use counts
/// are its own, starting at zero (pp.323-324); a subform named as both a
/// leader and a trailer spends one count for both (p.322).
pub(super) struct Overflow {
    leaders: Vec<Target>,
    trailers: Vec<Target>,
    used: HashMap<Vec<usize>, u32>,
}

struct Target {
    path: Vec<usize>,
    /// Most uses; a leader with no `occur` element is unlimited (gap G-L17).
    max: u32,
    /// Its instances, laid once, stacked top to bottom.
    parts: Vec<Placed>,
}

impl Overflow {
    /// The `overflow` element `node` (at `path`) asserts, its targets
    /// resolved under the template element `root`. `None` when it asserts
    /// neither kind. Inheritance from a containing subform is later work.
    pub(super) fn of(
        ctx: &mut Ctx<'_>,
        root: &XfaNode,
        node: &XfaNode,
        path: &[usize],
    ) -> Option<Self> {
        let o = node.child("overflow")?;
        let mut list = |a| {
            o.attr(a)
                .map(|v| targets(ctx, root, v, path))
                .unwrap_or_default()
        };
        let (leaders, trailers) = (list("leader"), list("trailer"));
        if leaders.is_empty() && trailers.is_empty() {
            return None;
        }
        Some(Self {
            leaders,
            trailers,
            used: HashMap::new(),
        })
    }

    fn next(&self, leader: bool) -> Option<&Target> {
        let list = if leader {
            &self.leaders
        } else {
            &self.trailers
        };
        list.iter()
            .find(|t| self.used.get(&t.path).copied().unwrap_or(0) < t.max)
    }

    /// Height of the trailer the next overflow would lay: the space to
    /// reserve now (p.320, gap G-L21).
    pub(super) fn trailer_height(&self) -> f64 {
        self.next(false)
            .map_or(0.0, |t| t.parts.iter().map(|p| p.rect.h).sum())
    }

    /// Spend one use of the next leader (or trailer) and return its parts.
    /// Each copy spends its items against [`super::MAX_LAYOUT_ITEMS`].
    pub(super) fn take(&mut self, ctx: &mut Ctx<'_>, leader: bool) -> Vec<Placed> {
        let Some(t) = self.next(leader) else {
            return Vec::new();
        };
        let (path, parts) = (t.path.clone(), t.parts.clone());
        let cost: usize = parts.iter().map(|p| p.items.len()).sum();
        if ctx.items.saturating_add(cost) > super::MAX_LAYOUT_ITEMS {
            ctx.warn(LayoutWarning::ItemLimit);
            return Vec::new();
        }
        ctx.items += cost;
        *self.used.entry(path).or_insert(0) += 1;
        parts
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
fn find<'t>(root: &'t XfaNode, spec: &str) -> Option<(&'t XfaNode, Vec<usize>)> {
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
    /// Lay `parts` one after another in the current area's flow.
    pub(super) fn put_parts(&mut self, parts: Vec<Placed>, ca: super::LayoutRect) {
        for p in parts {
            let at = self.flow.slot(p.rect.w);
            self.flow.commit(p.rect.w, p.rect.h);
            self.emit(p, ca, at);
        }
    }
}

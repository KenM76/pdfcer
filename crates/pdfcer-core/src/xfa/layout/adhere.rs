//! Adhesion, `keep@next` / `keep@previous` (XFA 3.3 Ch.8 pp.311-314).

use std::collections::VecDeque;

use super::LayoutWarning;
use super::geometry::{Placed, instances, is_placeable};
use super::split::split;
use crate::xfa::{LayoutStrategy, XfaNode};

/// The adhering edges of one instance. `pageArea` is laid as `contentArea`
/// for now: at least as strict, so a pair it keeps together is also on one
/// page (gap G-L12).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Keep {
    pub(super) next: bool,
    pub(super) previous: bool,
}

impl Keep {
    pub(super) fn of(node: &XfaNode) -> Self {
        let set = |a| {
            node.child("keep")
                .and_then(|k| k.attr(a))
                .is_some_and(|v| matches!(v.trim(), "contentArea" | "pageArea"))
        };
        Self {
            next: set("next"),
            previous: set("previous"),
        }
    }
}

/// Whether `a` then `b` adhere: either side's keep says so (p.312).
pub(super) fn adheres(a: Keep, b: Keep) -> bool {
    a.next || b.previous
}

/// Whether `node` asserts a break that moves to another region. An
/// explicit break wins over adhesion (gap G-L27).
pub(super) fn hard_break(node: &XfaNode, which: &str) -> bool {
    node.child(which).is_some_and(|b| {
        b.attr("targetType")
            .is_some_and(|t| t.trim() != "auto" && !t.trim().is_empty())
    })
}

/// Whether the adhering `chain` can start in `room`: every member before
/// the first one that splits there fits whole, and that one's top fragment
/// fits (p.313; derived algorithm, see the spec RAG's adhesion entry).
pub(super) fn chain_fits(chain: &[&Placed], mut room: f64) -> bool {
    for p in chain {
        if p.rect.h <= room {
            room -= p.rect.h;
            continue;
        }
        return split((*p).clone(), room).is_ok();
    }
    true
}

/// One top-level instance awaiting placement.
struct Pending {
    placed: Placed,
    /// Does it adhere to the instance before it?
    glued: bool,
}

impl<'t> super::Run<'t, '_> {
    /// Lay the root's children in order, with their breaks. Adjacent
    /// instances that adhere (a `subformSet` is transparent, Ch.9 p.357) are
    /// held off together. Only a `tb` root enforces adhesion yet; an
    /// explicit break wins (gap G-L27).
    pub(super) fn place_children(&mut self, root: &'t XfaNode, root_path: &[usize]) {
        let enforce = self.root_strategy == LayoutStrategy::Tb;
        let (mut meta, mut queue) = (Vec::new(), VecDeque::new());
        let mut prev: Option<(&XfaNode, Keep)> = None;
        for (i, c) in root.children.iter().enumerate() {
            if !is_placeable(c) {
                continue;
            }
            let mut path = root_path.to_vec();
            path.push(i);
            let mut placed = Vec::new();
            instances(&mut self.ctx, c, &path, self.root_strategy, &mut placed);
            meta.push((c, path, placed.len()));
            for p in placed {
                let glued = prev.is_some_and(|(n, k)| {
                    enforce
                        && adheres(k, p.keep)
                        && (std::ptr::eq(n, c)
                            || !(hard_break(n, "breakAfter") || hard_break(c, "breakBefore")))
                });
                prev = Some((c, p.keep));
                queue.push_back(Pending { placed: p, glued });
            }
        }
        for (node, path, n) in meta {
            self.apply_break(node, &path, "breakBefore");
            let mut alive = true;
            for _ in 0..n {
                let Some(p) = queue.pop_front() else { break };
                if alive && !self.put_adhering(p, &path, &queue) {
                    self.ctx
                        .warn(LayoutWarning::ContentDiscarded { path: path.clone() });
                    alive = false;
                }
            }
            if alive {
                self.apply_break(node, &path, "breakAfter");
            }
        }
    }

    /// [`Self::put`], first moving to the next area when `p` heads an
    /// adhering chain (continued in `rest`) that cannot start in the rest of
    /// a non-empty one.
    fn put_adhering(&mut self, p: Pending, path: &[usize], rest: &VecDeque<Pending>) -> bool {
        let heads = !p.glued && rest.front().is_some_and(|n| n.glued);
        if heads
            && let Some(ca) = self.content_area()
            && !self.current_empty()
        {
            let room = ca.h - self.flow.slot(p.placed.rect.w).1;
            let chain: Vec<&Placed> = std::iter::once(&p.placed)
                .chain(rest.iter().take_while(|n| n.glued).map(|n| &n.placed))
                .collect();
            if !chain_fits(&chain, room) {
                self.area_index += 1;
            }
        }
        self.put(p.placed, path, p.glued)
    }
}

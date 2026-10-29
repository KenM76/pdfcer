//! Adhesion, `keep@next` / `keep@previous` (XFA 3.3 Ch.8 pp.311-314).

use std::collections::VecDeque;

use super::LayoutWarning;
use super::bookend::bookend;
use super::geometry::{Ctx, Placed, instances, is_placeable};
use super::split::split;
use crate::xfa::{LayoutStrategy, XfaNode};

/// How `keep@next`/`keep@previous`="pageArea" is laid. XFA 3.3 defines it
/// as "on the same page" (Ch.8 p.312); Adobe's processor has never
/// supported it and treats it as `none` (Ch.28 p.1229, gap G-L12). Either
/// way a [`LayoutWarning::PageAreaKeep`] discloses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum PageAreaKeep {
    /// The adhering pair may be parted between content areas of one page,
    /// never between pages.
    #[default]
    Spec,
    /// No adhesion, as Adobe lays it.
    Adobe,
}

/// How strongly two neighbours adhere, weakest first. Where the two sides
/// disagree the stricter `contentArea` governs (gap G-L13).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Hold {
    #[default]
    None,
    /// Same page.
    Page,
    /// Same content area.
    Area,
}

/// The adhering edges of one instance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Keep {
    pub(super) next: Hold,
    pub(super) previous: Hold,
}

impl Keep {
    /// The `keep@next`/`keep@previous` edges `node` (at `path`) asserts,
    /// `pageArea` laid per [`PageAreaKeep`] and disclosed.
    pub(super) fn of(ctx: &mut Ctx<'_>, node: &XfaNode, path: &[usize]) -> Self {
        let mut page = false;
        let mut set = |a| match node.child("keep").and_then(|k| k.attr(a)).map(str::trim) {
            Some("contentArea") => Hold::Area,
            Some("pageArea") => {
                page = true;
                match ctx.opts.page_area_keep {
                    PageAreaKeep::Spec => Hold::Page,
                    PageAreaKeep::Adobe => Hold::None,
                }
            }
            _ => Hold::None,
        };
        let keep = Self {
            next: set("next"),
            previous: set("previous"),
        };
        if page {
            ctx.warn(LayoutWarning::PageAreaKeep {
                path: path.to_vec(),
            });
        }
        keep
    }
}

/// How `a` then `b` adhere: either side's keep says so (p.312).
pub(super) fn adheres(a: Keep, b: Keep) -> Hold {
    a.next.max(b.previous)
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
pub(super) fn chain_fits(chain: &[&Placed], mut room: f64, page_end: bool) -> bool {
    for p in chain {
        if p.rect.h <= room {
            room -= p.rect.h;
            continue;
        }
        return split((*p).clone(), room, page_end).is_ok();
    }
    true
}

/// Does a pair adhering by `h` have to stay together at the end of a
/// content area (`page_end`: the last on its page)?
pub(super) fn holds(h: Hold, page_end: bool) -> bool {
    h == Hold::Area || h == Hold::Page && page_end
}

/// One top-level instance awaiting placement.
struct Pending {
    placed: Placed,
    /// How it adheres to the instance before it.
    glued: Hold,
}

impl<'t: 'o, 'o> super::Run<'t, 'o> {
    /// Lay the root's children in order, with their breaks. Adjacent
    /// instances that adhere (a `subformSet` is transparent, Ch.9 p.357) are
    /// held off together. Only a `tb` root enforces adhesion yet; an
    /// explicit break wins (gap G-L27).
    pub(super) fn place_children(&mut self, root: &'t XfaNode, root_path: &[usize]) {
        let enforce = self.root_strategy == LayoutStrategy::Tb;
        let (mut meta, mut queue) = (Vec::new(), VecDeque::new());
        let mut prev: Option<(&XfaNode, Keep)> = None;
        for (c, path, placed) in self.root_entries(root, root_path) {
            meta.push((c, path, placed.len()));
            for p in placed {
                let glued = prev
                    .filter(|&(n, _)| {
                        enforce
                            && (std::ptr::eq(n, c)
                                || !(hard_break(n, "breakAfter") || hard_break(c, "breakBefore")))
                    })
                    .map_or(Hold::None, |(_, k)| adheres(k, p.keep));
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

    /// The root's children with their instances, between the root's
    /// bookend leader and trailer: the start and end of the whole document
    /// (Ch.8 p.318).
    fn root_entries(
        &mut self,
        root: &'t XfaNode,
        root_path: &[usize],
    ) -> Vec<(&'o XfaNode, Vec<usize>, Vec<Placed>)> {
        let strategy = self.root_strategy;
        let mut out: Vec<_> = bookend(&mut self.ctx, root, root_path, true, strategy)
            .into_iter()
            .collect();
        for (i, c) in root.children.iter().enumerate() {
            if !is_placeable(c) {
                continue;
            }
            let mut path = root_path.to_vec();
            path.push(i);
            let mut placed = Vec::new();
            instances(&mut self.ctx, c, &path, strategy, &mut placed);
            out.push((c, path, placed));
        }
        out.extend(bookend(&mut self.ctx, root, root_path, false, strategy));
        out
    }

    /// [`Self::put`], first moving to the next area when `p` heads an
    /// adhering chain (continued in `rest`) that cannot start in the rest of
    /// a non-empty one.
    fn put_adhering(&mut self, p: Pending, path: &[usize], rest: &VecDeque<Pending>) -> bool {
        let pe = self.page_ends_here();
        let heads = !holds(p.glued, pe) && rest.front().is_some_and(|n| holds(n.glued, pe));
        if heads
            && let Some(ca) = self.content_area()
            && !self.current_empty()
        {
            let room = ca.h - self.flow.slot(p.placed.rect.w).1;
            let chain: Vec<&Placed> = std::iter::once(&p.placed)
                .chain(
                    rest.iter()
                        .take_while(|n| holds(n.glued, self.page_ends_here()))
                        .map(|n| &n.placed),
                )
                .collect();
            if !chain_fits(&chain, room, self.page_ends_here()) {
                self.area_index += 1;
            }
        }
        let glued = holds(p.glued, self.page_ends_here());
        self.put(p.placed, path, glued)
    }

    /// Is the current content area the last on its page?
    pub(super) fn page_ends_here(&self) -> bool {
        self.area_index + 1 >= self.filled.len()
    }
}

//! Bookend leaders and trailers, `bookend@leader` / `bookend@trailer`
//! (XFA 3.3 Ch.8 pp.317-318, 326-327; Ch.17 p.625).

use super::LayoutWarning;
use super::geometry::{Ctx, Placed, instances};
use super::overflow::find;
use crate::xfa::{LayoutStrategy, XfaNode};

/// The bookend leader (or trailer) `node` asserts, laid by `strategy` as
/// its first (or last) content: once per instance, so a split puts the
/// leader in the first fragment and the trailer in the last (p.318). It is
/// ordinary content, with no reservation, inheritance or occurrence limit
/// (gap G-L25).
///
/// A target naming no single `subform`/`subformSet`, or one already being
/// laid as a bookend further up (a cycle), is skipped with
/// [`LayoutWarning::InvalidBookendTarget`].
pub(super) fn bookend<'o>(
    ctx: &mut Ctx<'o>,
    node: &XfaNode,
    path: &[usize],
    leader: bool,
    strategy: LayoutStrategy,
) -> Option<(&'o XfaNode, Vec<usize>, Vec<Placed>)> {
    let spec = node
        .child("bookend")?
        .attr(if leader { "leader" } else { "trailer" })?
        .trim();
    if spec.is_empty() {
        return None;
    }
    let root = ctx.root;
    let found = find(root, spec).filter(|(_, at)| !ctx.bookends.contains(at));
    let Some((target, at)) = found else {
        ctx.warn(LayoutWarning::InvalidBookendTarget {
            path: path.to_vec(),
            target: spec.to_owned(),
        });
        return None;
    };
    ctx.bookends.push(at.clone());
    let mut placed = Vec::new();
    instances(ctx, target, &at, strategy, &mut placed);
    ctx.bookends.pop();
    Some((target, at, placed))
}

/// `kids` with `node`'s bookend leader before and trailer after.
pub(super) fn wrap(
    ctx: &mut Ctx<'_>,
    node: &XfaNode,
    path: &[usize],
    strategy: LayoutStrategy,
    kids: Vec<Placed>,
) -> Vec<Placed> {
    if node.child("bookend").is_none() {
        return kids;
    }
    let mut out = bookend(ctx, node, path, true, strategy).map_or_else(Vec::new, |b| b.2);
    out.extend(kids);
    if let Some((_, _, t)) = bookend(ctx, node, path, false, strategy) {
        out.extend(t);
    }
    out
}

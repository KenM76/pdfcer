//! Positioned placement of one template subtree (XFA 3.3 Ch.7 pp.264-268,
//! Ch.8 pp.275-279; box model Ch.2 pp.49-55).

use super::adhere::{Hold, Keep, adheres};
use super::flow::{Flow, is_flow};
use super::lines::{Lines, content};
use super::{LayoutItem, LayoutOptions, LayoutRect, LayoutWarning, NegativeCoordinates};
use crate::xfa::{AnchorType, ContainerKind, LayoutStrategy, Measurement, Presence, XfaNode};

/// `font@size` default (Ch.17 p.741), the base of an `em` measurement.
const DEFAULT_FONT_SIZE: f64 = 10.0;

/// Shared state of one layout run.
pub(super) struct Ctx<'o> {
    pub(super) opts: &'o LayoutOptions,
    pub(super) warnings: Vec<LayoutWarning>,
    pub(super) items: usize,
    /// The template element: where bookend targets are looked up.
    pub(super) root: &'o XfaNode,
    /// Paths of the bookend targets being laid, outermost first: the
    /// cycle guard.
    pub(super) bookends: Vec<Vec<usize>>,
}

impl Ctx<'_> {
    /// Record a layout warning.
    pub(super) fn warn(&mut self, w: LayoutWarning) {
        if !self.warnings.contains(&w) {
            self.warnings.push(w);
        }
    }

    /// A measurement in points; `em` against the default font size, `%`
    /// unresolved (no property in this pass gives it a base).
    pub(super) fn length(
        &mut self,
        node: &XfaNode,
        path: &[usize],
        attr: &'static str,
    ) -> Option<f64> {
        match node.measurement(attr)? {
            Measurement::Points(v) => Some(v),
            Measurement::Em(v) => Some(v * DEFAULT_FONT_SIZE),
            _ => {
                self.warn(LayoutWarning::UnresolvedMeasurement {
                    path: path.to_vec(),
                    attribute: attr,
                });
                None
            }
        }
    }
}

/// Child elements that take part in layout as containers.
pub(super) fn is_placeable(node: &XfaNode) -> bool {
    matches!(
        node.container_kind(),
        Some(
            ContainerKind::Subform
                | ContainerKind::SubformSet
                | ContainerKind::Area
                | ContainerKind::ExclGroup
                | ContainerKind::Field
                | ContainerKind::Draw
        )
    )
}

/// One placed instance: its rectangle in the parent's content region (x/y
/// zero when the parent flows) and its items relative to that rectangle.
#[derive(Clone)]
pub(super) struct Placed {
    pub(super) rect: LayoutRect,
    pub(super) items: Vec<LayoutItem>,
    /// Per item, what a split line must respect.
    pub(super) flags: Vec<ItemFlags>,
    pub(super) keep: Keep,
    /// Template path of the instance's node.
    pub(super) path: Vec<usize>,
}

/// What a split line must respect at one item.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct ItemFlags {
    /// May a split line cut it (a `tb` subform whose `keep@intact` is
    /// `none`, Ch.8 pp.281, 291, or a text container with `lines`)?
    pub(super) breakable: bool,
    /// The line grid a split must fall on, for a field or draw.
    pub(super) lines: Option<Lines>,
    /// How many items back its previous `tb` sibling sits, when the two
    /// adhere (Ch.8 pp.311-314); 0 when it adheres to none.
    pub(super) glue: usize,
    /// How strongly it adheres to that sibling.
    pub(super) hold: Hold,
}

/// Lay `node` (at `path`) positioned at `origin`, appending its items.
pub(super) fn lay(
    ctx: &mut Ctx<'_>,
    node: &XfaNode,
    path: &[usize],
    origin: (f64, f64),
    out: &mut Vec<LayoutItem>,
) {
    let mut placed = Vec::new();
    instances(ctx, node, path, LayoutStrategy::Position, &mut placed);
    for p in placed {
        out.extend(shifted(p.items, origin.0 + p.rect.x, origin.1 + p.rect.y));
    }
}

/// The instances of `node` in a parent laid by `parent`.
///
/// A `subformSet` has no geometry: its children's instances are its own.
/// `hidden`/`inactive` content takes no space and is omitted; an empty
/// merge instantiates each subform `occur@initial` times (Ch.17 p.786).
/// Items are emitted parent first (Z-order, p.70). Every instance, a
/// `subformSet`'s included, spends one unit of [`super::MAX_LAYOUT_ITEMS`].
pub(super) fn instances(
    ctx: &mut Ctx<'_>,
    node: &XfaNode,
    path: &[usize],
    parent: LayoutStrategy,
    out: &mut Vec<Placed>,
) {
    if matches!(node.presence(), Presence::Hidden | Presence::Inactive) {
        return;
    }
    let copies = node.occur().map_or(1, |o| o.initial);
    for _ in 0..copies {
        if ctx.items >= super::MAX_LAYOUT_ITEMS {
            ctx.warn(LayoutWarning::ItemLimit);
            return;
        }
        if node.container_kind() == Some(ContainerKind::SubformSet) {
            ctx.items += 1;
            let mut kids = Vec::new();
            children(ctx, node, path, parent, &mut kids);
            out.extend(super::bookend::wrap(ctx, node, path, parent, kids));
        } else if let Some(p) = lay_one(ctx, node, path, parent) {
            out.push(p);
        }
    }
}

fn children(
    ctx: &mut Ctx<'_>,
    node: &XfaNode,
    path: &[usize],
    strategy: LayoutStrategy,
    out: &mut Vec<Placed>,
) {
    for (i, c) in node.children.iter().enumerate() {
        if is_placeable(c) {
            let mut p = path.to_vec();
            p.push(i);
            instances(ctx, c, &p, strategy, out);
        }
    }
}

/// `items` moved by (`dx`, `dy`).
pub(super) fn shifted(
    items: Vec<LayoutItem>,
    dx: f64,
    dy: f64,
) -> impl Iterator<Item = LayoutItem> {
    items.into_iter().map(move |mut i| {
        i.rect.x += dx;
        i.rect.y += dy;
        i
    })
}

/// The strategy `node` lays its children by. Gap G-L2 (derived reading): a
/// subform uses its own `layout`, an area or exclGroup inherits.
fn strategy_of(
    ctx: &mut Ctx<'_>,
    node: &XfaNode,
    path: &[usize],
    kind: ContainerKind,
    parent: LayoutStrategy,
) -> LayoutStrategy {
    let s = match kind {
        ContainerKind::Subform => node.layout(),
        ContainerKind::Area | ContainerKind::ExclGroup => parent,
        _ => LayoutStrategy::Position,
    };
    if s != LayoutStrategy::Position && !is_flow(s) {
        ctx.warn(LayoutWarning::FlowedLayoutPending {
            path: path.to_vec(),
            strategy: s,
        });
    }
    s
}

/// One container in a parent laid by `parent`.
fn lay_one(
    ctx: &mut Ctx<'_>,
    node: &XfaNode,
    path: &[usize],
    parent: LayoutStrategy,
) -> Option<Placed> {
    let kind = node.container_kind()?;
    if ctx.items >= super::MAX_LAYOUT_ITEMS {
        ctx.warn(LayoutWarning::ItemLimit);
        return None;
    }
    ctx.items += 1;
    let strategy = strategy_of(ctx, node, path, kind, parent);
    let inset = |ctx: &mut Ctx<'_>, a| {
        node.child("margin")
            .and_then(|m| ctx.length(m, path, a))
            .unwrap_or(0.0)
    };
    let (left, top) = (inset(ctx, "leftInset"), inset(ctx, "topInset"));
    let (right, bottom) = (inset(ctx, "rightInset"), inset(ctx, "bottomInset"));
    let mut kids = Vec::new();
    children(ctx, node, path, strategy, &mut kids);
    if kind == ContainerKind::Subform {
        kids = super::bookend::wrap(ctx, node, path, strategy, kids);
    }
    let wrap = (ctx.length(node, path, "w"))
        .or_else(|| ctx.length(node, path, "maxW"))
        .map(|w| (w - left - right).max(0.0));
    let (laid, kid_breaks) = arrange(kids, strategy, wrap, (left, top));
    let (ex, ey) = laid.iter().fold((0.0_f64, 0.0_f64), |(x, y), i| {
        (x.max(i.rect.x + i.rect.w), y.max(i.rect.y + i.rect.h))
    });
    let (w, h) = size(ctx, node, path, kind, (ex + right, ey + bottom));
    let rect = position(ctx, node, path, parent, (w, h));
    let mut items = vec![LayoutItem {
        kind,
        name: node.name().map(str::to_owned),
        path: path.to_vec(),
        rect: LayoutRect {
            x: 0.0,
            y: 0.0,
            w,
            h,
        },
        background: false,
        visible: node.presence() == Presence::Visible,
        content_area: None,
        fragment: None,
    }];
    items.extend(laid);
    let lines = Lines::of(ctx, node, path, kind, h);
    let mut flags = vec![ItemFlags {
        breakable: breaks(node, kind, strategy) || lines.is_some(),
        lines,
        ..ItemFlags::default()
    }];
    flags.extend(kid_breaks);
    Some(Placed {
        rect,
        items,
        flags,
        keep: Keep::of(ctx, node, path),
        path: path.to_vec(),
    })
}

/// Where a `w` × `h` instance sits in its parent: at the origin when the
/// parent flows (p.279), else by `x`/`y` and `anchorType`.
fn position(
    ctx: &mut Ctx<'_>,
    node: &XfaNode,
    path: &[usize],
    parent: LayoutStrategy,
    (w, h): (f64, f64),
) -> LayoutRect {
    if is_flow(parent) {
        return LayoutRect {
            x: 0.0,
            y: 0.0,
            w,
            h,
        };
    }
    let (ax, ay) = anchor_fraction(node.anchor_type());
    let x = ctx.length(node, path, "x").unwrap_or(0.0) - ax * w;
    let y = ctx.length(node, path, "y").unwrap_or(0.0) - ay * h;
    place(ctx, path, LayoutRect { x, y, w, h })
}

/// A `tb` subform may be split unless `keep@intact` says otherwise. The
/// default keys on the subform's own layout, `none` for a flowing one (gap
/// G-L18, Ch.17 p.762). Fields and draws split by [`Lines::of`].
fn breaks(node: &XfaNode, kind: ContainerKind, strategy: LayoutStrategy) -> bool {
    let intact = node
        .child("keep")
        .and_then(|k| k.attr("intact"))
        .map_or("none", str::trim);
    kind == ContainerKind::Subform && strategy == LayoutStrategy::Tb && intact == "none"
}

/// Children's items relative to the container's top-left: positioned by
/// their own rectangles, or flowed from the margin corner. In a `tb`
/// container a child adhering to its previous sibling is glued to it
/// (Ch.8 pp.311-314; instances of one subform are siblings too).
fn arrange(
    kids: Vec<Placed>,
    strategy: LayoutStrategy,
    wrap: Option<f64>,
    (left, top): (f64, f64),
) -> (Vec<LayoutItem>, Vec<ItemFlags>) {
    let mut flow = Flow::new(strategy, wrap);
    let (mut out, mut flags) = (Vec::new(), Vec::new());
    let mut prev: Option<(usize, Keep)> = None;
    for k in kids {
        let start = flags.len();
        let mut kf = k.flags;
        if let (Some((at, keep)), Some(f)) = (prev, kf.first_mut())
            && strategy == LayoutStrategy::Tb
            && adheres(keep, k.keep) > Hold::None
        {
            f.glue = start - at;
            f.hold = adheres(keep, k.keep);
        }
        prev = Some((start, k.keep));
        let (x, y) = if is_flow(strategy) {
            let at = flow.slot(k.rect.w);
            flow.commit(k.rect.w, k.rect.h);
            at
        } else {
            (k.rect.x, k.rect.y)
        };
        out.extend(shifted(k.items, left + x, top + y));
        flags.extend(kf);
    }
    (out, flags)
}

/// `node`'s width and height: `w`/`h` when given, else the content's
/// extent clamped to min/max (Ch.8 pp.275-277). A subform's content is
/// `laid`, its laid children; a field's or draw's is its text, wrapped at
/// `w` or a positive `maxW`.
fn size(
    ctx: &mut Ctx<'_>,
    node: &XfaNode,
    path: &[usize],
    kind: ContainerKind,
    laid: (f64, f64),
) -> (f64, f64) {
    let own = match kind {
        ContainerKind::Field | ContainerKind::Draw => {
            let wrap = ctx
                .length(node, path, "w")
                .or_else(|| ctx.length(node, path, "maxW").filter(|v| *v > 0.0));
            content(node, kind, wrap)
        }
        _ => Some(laid),
    };
    let w = extent(ctx, node, path, ["w", "minW", "maxW"], own.map(|c| c.0));
    let h = extent(ctx, node, path, ["h", "minH", "maxH"], own.map(|c| c.1));
    (w, h)
}

/// One axis: the given size, else the content extent clamped to min/max
/// (Ch.8 p.276; min > max warns and swaps; a max of 0 is omitted, 3.0),
/// else min.
fn extent(
    ctx: &mut Ctx<'_>,
    node: &XfaNode,
    path: &[usize],
    [size, min, max]: [&'static str; 3],
    content: Option<f64>,
) -> f64 {
    if let Some(v) = ctx.length(node, path, size) {
        return v;
    }
    let mut lo = ctx.length(node, path, min).unwrap_or(0.0);
    let mut hi = (ctx.length(node, path, max))
        .filter(|v| *v != 0.0)
        .unwrap_or(f64::INFINITY);
    if lo > hi {
        ctx.warn(LayoutWarning::MinGreaterThanMax {
            path: path.to_vec(),
            attribute: min,
        });
        std::mem::swap(&mut lo, &mut hi);
    }
    content.unwrap_or(lo).clamp(lo, hi)
}

/// The anchor point as a fraction of the object's width and height.
const fn anchor_fraction(a: AnchorType) -> (f64, f64) {
    match a {
        AnchorType::TopCenter => (0.5, 0.0),
        AnchorType::TopRight => (1.0, 0.0),
        AnchorType::MiddleLeft => (0.0, 0.5),
        AnchorType::MiddleCenter => (0.5, 0.5),
        AnchorType::MiddleRight => (1.0, 0.5),
        AnchorType::BottomLeft => (0.0, 1.0),
        AnchorType::BottomCenter => (0.5, 1.0),
        AnchorType::BottomRight => (1.0, 1.0),
        _ => (0.0, 0.0),
    }
}

/// Gap G-L1: a negative resolved coordinate is reported, then kept or
/// clamped per [`LayoutOptions::negative_coordinates`].
fn place(ctx: &mut Ctx<'_>, path: &[usize], mut r: LayoutRect) -> LayoutRect {
    if r.x < 0.0 || r.y < 0.0 {
        ctx.warn(LayoutWarning::NegativeCoordinate {
            path: path.to_vec(),
        });
        if ctx.opts.negative_coordinates == NegativeCoordinates::Clamp {
            r.x = r.x.max(0.0);
            r.y = r.y.max(0.0);
        }
    }
    r
}

//! Positioned placement of one template subtree (XFA 3.3 Ch.7 pp.264-268,
//! Ch.8 pp.275-279; box model Ch.2 pp.49-55).

use super::{LayoutItem, LayoutOptions, LayoutRect, LayoutWarning, NegativeCoordinates};
use crate::xfa::{AnchorType, ContainerKind, LayoutStrategy, Measurement, Presence, XfaNode};

/// `font@size` default (Ch.17 p.741), the base of an `em` measurement.
const DEFAULT_FONT_SIZE: f64 = 10.0;

/// Shared state of one layout run.
pub(super) struct Ctx<'o> {
    pub(super) opts: &'o LayoutOptions,
    pub(super) warnings: Vec<LayoutWarning>,
    pub(super) items: usize,
}

impl Ctx<'_> {
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

/// Lay `node` (at `path`) positioned in a container whose content region
/// starts at the origin, appending its items translated by `origin`.
///
/// A `subformSet` has no geometry: its children are laid in the enclosing
/// container. `hidden`/`inactive` content takes no space and is omitted; an
/// empty merge instantiates each subform `occur@initial` times (Ch.17
/// p.786). Items are emitted parent first (Z-order, p.70). Every instance,
/// a `subformSet`'s included, spends one unit of [`super::MAX_LAYOUT_ITEMS`].
pub(super) fn lay(
    ctx: &mut Ctx<'_>,
    node: &XfaNode,
    path: &[usize],
    origin: (f64, f64),
    out: &mut Vec<LayoutItem>,
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
            lay_children(ctx, node, path, origin, out);
        } else if let Some((rect, local)) = lay_one(ctx, node, path) {
            out.extend(local.into_iter().map(|mut i| {
                i.rect.x += origin.0 + rect.x;
                i.rect.y += origin.1 + rect.y;
                i
            }));
        }
    }
}

fn lay_children(
    ctx: &mut Ctx<'_>,
    node: &XfaNode,
    path: &[usize],
    origin: (f64, f64),
    out: &mut Vec<LayoutItem>,
) {
    for (i, c) in node.children.iter().enumerate() {
        if is_placeable(c) {
            let mut p = path.to_vec();
            p.push(i);
            lay(ctx, c, &p, origin, out);
        }
    }
}

/// One container: its rectangle relative to the parent's content region,
/// and its items relative to its own top-left.
fn lay_one(
    ctx: &mut Ctx<'_>,
    node: &XfaNode,
    path: &[usize],
) -> Option<(LayoutRect, Vec<LayoutItem>)> {
    let kind = node.container_kind()?;
    if ctx.items >= super::MAX_LAYOUT_ITEMS {
        ctx.warn(LayoutWarning::ItemLimit);
        return None;
    }
    ctx.items += 1;
    if kind == ContainerKind::Subform && node.layout() != LayoutStrategy::Position {
        ctx.warn(LayoutWarning::FlowedLayoutPending {
            path: path.to_vec(),
            strategy: node.layout(),
        });
    }
    let inset = |ctx: &mut Ctx<'_>, a| {
        node.child("margin")
            .and_then(|m| ctx.length(m, path, a))
            .unwrap_or(0.0)
    };
    let (left, top) = (inset(ctx, "leftInset"), inset(ctx, "topInset"));
    let (right, bottom) = (inset(ctx, "rightInset"), inset(ctx, "bottomInset"));
    let mut children = Vec::new();
    lay_children(ctx, node, path, (left, top), &mut children);
    let (ex, ey) = children.iter().fold((0.0_f64, 0.0_f64), |(x, y), i| {
        (x.max(i.rect.x + i.rect.w), y.max(i.rect.y + i.rect.h))
    });
    let grows = !matches!(kind, ContainerKind::Field | ContainerKind::Draw);
    let w = extent(
        ctx,
        node,
        path,
        ["w", "minW", "maxW"],
        grows.then_some(ex + right),
    );
    let h = extent(
        ctx,
        node,
        path,
        ["h", "minH", "maxH"],
        grows.then_some(ey + bottom),
    );
    let (ax, ay) = anchor_fraction(node.anchor_type());
    let x = ctx.length(node, path, "x").unwrap_or(0.0) - ax * w;
    let y = ctx.length(node, path, "y").unwrap_or(0.0) - ay * h;
    let rect = place(ctx, path, LayoutRect { x, y, w, h });
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
    }];
    items.extend(children);
    Some((rect, items))
}

/// One axis: the given size, else the content extent clamped to min/max
/// (Ch.8 p.276; min > max warns and swaps), else min.
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
    let mut hi = ctx.length(node, path, max).unwrap_or(f64::INFINITY);
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

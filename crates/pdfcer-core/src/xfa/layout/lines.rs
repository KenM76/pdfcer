//! Where a growable text field or draw may split: between its lines (XFA
//! 3.3 Ch.8 pp.290-292; `multiLine` p.866, `para` orphans/widows p.800).

use super::geometry::Ctx;
use crate::xfa::paint::{line_pitch, widget};
use crate::xfa::{ContainerKind, Presence, XfaNode};

/// Rounding allowance when a split line is compared with a line boundary.
const SNAP: f64 = 1e-6;

/// Where one piece of a split container lies in the whole container.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Fragment {
    /// Points from the whole container's top down to this piece's top.
    pub offset: f64,
    /// The whole container's height in points.
    pub height: f64,
}

impl Fragment {
    /// The piece's whole container's fragment, starting `d` points further
    /// down: the tail of a cut at `d` below the piece's top.
    pub(super) fn tail_of(piece: Option<Self>, height: f64, d: f64) -> Self {
        let whole = piece.unwrap_or(Self {
            offset: 0.0,
            height,
        });
        Self {
            offset: whole.offset + d,
            ..whole
        }
    }

    /// The fragment of a piece cut from the top of an item `height` tall.
    pub(super) fn head_of(piece: Option<Self>, height: f64) -> Self {
        piece.unwrap_or(Self {
            offset: 0.0,
            height,
        })
    }
}

/// The line grid of a splittable text container, from its top.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Lines {
    /// The first line's top.
    top: f64,
    pitch: f64,
    /// The text region's bottom: the split never enters the margin below.
    bottom: f64,
    /// Lines a split leaves above it (`orphans`, at least one).
    above: f64,
    /// Lines a split leaves below it (`widows`, at least one).
    below: f64,
}

impl Lines {
    /// The grid of `node` (a field or draw `h` tall at `path`) when it may
    /// split: growable in height, `keep@intact="none"` (a field's default,
    /// a draw's is `contentArea`, p.292), multi-line text (`textEdit`
    /// `multiLine`, `0` in a field and `1` in a draw by default), unrotated,
    /// and any `top`/`bottom` caption sized by a positive `reserve`.
    pub(super) fn of(
        ctx: &mut Ctx<'_>,
        node: &XfaNode,
        path: &[usize],
        kind: ContainerKind,
        h: f64,
    ) -> Option<Self> {
        let draw = match kind {
            ContainerKind::Field => false,
            ContainerKind::Draw => true,
            _ => return None,
        };
        let intact = node.child("keep").and_then(|k| k.attr("intact"));
        let intact = intact.map_or(if draw { "contentArea" } else { "none" }, str::trim);
        let rotated = node
            .attr("rotate")
            .is_some_and(|r| r.trim().parse() != Ok(0));
        if node.attr("h").is_some() || intact != "none" || rotated || !multi_line(node, draw) {
            return None;
        }
        let mut inset = |n: Option<&XfaNode>, a| {
            n.and_then(|m| m.child("margin"))
                .and_then(|m| ctx.length(m, path, a))
                .unwrap_or(0.0)
        };
        let w = widget(node);
        let top = inset(Some(node), "topInset") + inset(w, "topInset");
        let bottom = h - inset(Some(node), "bottomInset") - inset(w, "bottomInset");
        let (cap_top, cap_bottom) = caption(ctx, node, path)?;
        let para = node.child("para");
        let count = |a| {
            para.and_then(|p| p.attr(a))
                .and_then(|v| v.trim().parse::<u32>().ok())
                .map_or(1.0, |n| f64::from(n.max(1)))
        };
        let pitch = line_pitch(node.child("font"), para);
        (pitch > SNAP).then_some(Self {
            top: top + cap_top,
            pitch,
            bottom: bottom - cap_bottom,
            above: count("orphans"),
            below: count("widows"),
        })
    }

    /// The lowest line at or above `s` (from the container's top) a split
    /// may take, if any.
    pub(super) fn at_or_above(&self, s: f64) -> Option<f64> {
        let fit = ((s - self.top) / self.pitch + SNAP).floor();
        let room = ((self.bottom - self.top) / self.pitch - self.below + SNAP).floor();
        let k = fit.min(room);
        (k >= self.above).then_some(self.top + k * self.pitch)
    }

    /// The grid of the part below a split `d` from the top, on a line.
    pub(super) fn after(self, d: f64) -> Self {
        Self {
            top: (self.top - d).max(0.0),
            bottom: self.bottom - d,
            ..self
        }
    }
}

/// Whether `node`'s content is text that may run to several lines.
fn multi_line(node: &XfaNode, draw: bool) -> bool {
    let text_value = node.child("value").is_none_or(|v| {
        v.children
            .iter()
            .all(|c| matches!(c.tag.as_str(), "text" | "exData"))
    });
    let lines = match widget(node) {
        None => draw,
        Some(w) if w.tag == "textEdit" => w.attr("multiLine").map_or(draw, |m| m.trim() == "1"),
        Some(_) => false,
    };
    text_value && lines
}

/// The space a `top` and a `bottom` caption take; `None` when one is not
/// sized by a positive `reserve` (its text extent is a painting matter).
fn caption(ctx: &mut Ctx<'_>, node: &XfaNode, path: &[usize]) -> Option<(f64, f64)> {
    let Some(c) = node
        .child("caption")
        .filter(|c| matches!(c.presence(), Presence::Visible | Presence::Invisible))
    else {
        return Some((0.0, 0.0));
    };
    let side = c.attr("placement").map(str::trim);
    if !matches!(side, Some("top" | "bottom")) {
        return Some((0.0, 0.0));
    }
    let reserve = ctx.length(c, path, "reserve").filter(|r| *r > 0.0)?;
    Some(if side == Some("top") {
        (reserve, 0.0)
    } else {
        (0.0, reserve)
    })
}

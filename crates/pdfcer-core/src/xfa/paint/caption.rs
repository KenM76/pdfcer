//! A field's, draw's or exclGroup's `caption`: its region carved from the
//! content region and its plain text set there (XFA 3.3 caption pp.643-644,
//! box model pp.50-51; slice `adobe_ext__xfa_appearance_drawing.md` §4).

use super::text::{has_unplain_value, plain_text};
use super::{CaptionFont, LayoutItem, LayoutRect, Painter, Presence, XfaNode, inset};

/// Where a caption sits against the rest of the content region.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// The side of a laid-out (non-`inline`) caption; `None` for `inline`.
pub(super) fn side(caption: &XfaNode) -> Option<Side> {
    match caption.attr("placement").map(str::trim) {
        Some("right") => Some(Side::Right),
        Some("top") => Some(Side::Top),
        Some("bottom") => Some(Side::Bottom),
        Some("inline") => None,
        _ => Some(Side::Left),
    }
}

/// Whether the caption takes space: `hidden` and `inactive` do not (p.68).
pub(super) fn occupies(caption: &XfaNode) -> bool {
    matches!(caption.presence(), Presence::Visible | Presence::Invisible)
}

/// Whether `node` has a visible caption this cut does not draw: an
/// `inline` one, or one holding rich text or other non-text, non-image
/// content.
pub(super) fn left_out(node: &XfaNode) -> bool {
    node.child("caption").is_some_and(|c| {
        c.presence() == Presence::Visible && (side(c).is_none() || has_unplain_value(c))
    })
}

impl Painter<'_> {
    /// Draw `node`'s caption in its region of `r` and return the rest of `r`
    /// (p.51). The region is `reserve` wide (left/right) or tall (top/bottom);
    /// with no positive `reserve` it is just enough for the unwrapped caption
    /// text plus the caption's margins (p.644; `-1` read as omitted, G-D7).
    pub(super) fn caption(
        &mut self,
        item: &LayoutItem,
        node: &XfaNode,
        r: LayoutRect,
    ) -> LayoutRect {
        self.caption_leaving(item, node, r, None)
    }

    /// [`Self::caption`], except that with no positive `reserve` and
    /// `leave` given, the caption takes all of `r` but `leave` along its
    /// placement axis (G-CB14's remainder reading).
    pub(super) fn caption_leaving(
        &mut self,
        item: &LayoutItem,
        node: &XfaNode,
        r: LayoutRect,
        leave: Option<f64>,
    ) -> LayoutRect {
        let Some(c) = node.child("caption").filter(|c| occupies(c)) else {
            return r;
        };
        let Some(side) = side(c) else {
            return r;
        };
        let text = plain_text(c).filter(|_| !has_unplain_value(c));
        let image = super::image::value_image(c);
        let font = match self.opts.caption_font {
            CaptionFont::Field => c.child("font").or_else(|| node.child("font")),
            _ => c.child("font"),
        };
        let para = c.child("para");
        let margin = c.child("margin");
        let horizontal = matches!(side, Side::Left | Side::Right);
        let size = super::length(c, "reserve")
            .filter(|v| *v > 0.0)
            .or_else(|| leave.map(|l| if horizontal { r.w - l } else { r.h - l }))
            .unwrap_or_else(|| {
                let (w, h) = match image {
                    Some(i) => self.image_extent(i).unwrap_or((0.0, 0.0)),
                    None => text.map_or((0.0, 0.0), |t| self.extent(item, t, font, para)),
                };
                // An empty rect inset by the margin has extent -(near + far).
                let m = inset(LayoutRect::default(), margin);
                if horizontal { w - m.w } else { h - m.h }
            });
        let (cap, rest) = split(r, side, size);
        if c.presence() == Presence::Visible {
            if let Some(i) = image {
                self.image(item, i, para, inset(cap, margin));
            } else if let Some(t) = text {
                self.set(item, t, font, para, inset(cap, margin));
            }
        }
        rest
    }
}

/// `r` split into the caption's `size`-deep strip on `side` and the rest;
/// the strip is clamped to `r`.
fn split(r: LayoutRect, side: Side, size: f64) -> (LayoutRect, LayoutRect) {
    let LayoutRect { x, y, w, h } = r;
    match side {
        Side::Left | Side::Right => {
            let s = size.clamp(0.0, w.max(0.0));
            let (cx, rx) = if side == Side::Left {
                (x, x + s)
            } else {
                (x + w - s, x)
            };
            (
                LayoutRect { x: cx, y, w: s, h },
                LayoutRect {
                    x: rx,
                    y,
                    w: w - s,
                    h,
                },
            )
        }
        Side::Top | Side::Bottom => {
            let s = size.clamp(0.0, h.max(0.0));
            let (cy, ry) = if side == Side::Top {
                (y, y + s)
            } else {
                (y + h - s, y)
            };
            (
                LayoutRect { x, y: cy, w, h: s },
                LayoutRect {
                    x,
                    y: ry,
                    w,
                    h: h - s,
                },
            )
        }
    }
}

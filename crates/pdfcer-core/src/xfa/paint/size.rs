//! The size a growable field's or draw's text asks for (XFA 3.3 Ch.8
//! pp.275-278; field auto-size Ch.2 p.51): the box model painting sets
//! text in, run outwards. The container's margins, its caption and a
//! field's widget margins are added to the text block. The border insets
//! independently of the content region (p.50) and adds nothing.

use super::caption::{Side, occupies, side};
use super::text::{has_unplain_value, plain_text, text_extent, widget};
use super::{LayoutRect, XfaNode, inset, length};

/// The narrowest positive wrap width: a region with no room left still
/// wraps, one word a line, rather than reading as unwrapped.
const MIN_WRAP: f64 = 1e-3;

/// The width and height `node`'s plain-text value needs, wrapped at
/// `wrap` (its nominal width) when given, one unwrapped line when
/// `single`; `None` when the value is empty or not plain text (an image's
/// size is not measured here), so the container keeps `minW`/`minH`.
pub(in crate::xfa) fn content_size(
    node: &XfaNode,
    wrap: Option<f64>,
    single: bool,
) -> Option<(f64, f64)> {
    if has_unplain_value(node) {
        return None;
    }
    let text = plain_text(node)?;
    let room = |m: Option<&XfaNode>| {
        let r = inset(LayoutRect::default(), m);
        (-r.w, -r.h)
    };
    let (mw, mh) = room(node.child("margin"));
    let (ww, wh) = room(widget(node).and_then(|w| w.child("margin")));
    let cap = caption_size(node);
    let (cw, ch) = match cap {
        Some((Side::Left | Side::Right, w, _)) => (w, 0.0),
        Some((Side::Top | Side::Bottom, _, h)) => (0.0, h),
        None => (0.0, 0.0),
    };
    let around_w = mw + ww + cw;
    let wrap_at = wrap.map_or(0.0, |w| (w - around_w).max(MIN_WRAP));
    let (tw, th) = text_extent(
        text,
        node.child("font"),
        node.child("para"),
        wrap_at,
        single,
    );
    let mut h = th + wh;
    if let Some((Side::Left | Side::Right, _, cap_h)) = cap {
        h = h.max(cap_h);
    }
    Some((tw + around_w, h + mh + ch))
}

/// `node`'s laid-out caption: its side, and the width and height it takes
/// (`reserve` along its axis when positive, else its unwrapped text plus
/// the caption's margins, p.644; set in its own `font`, gap G-D11's
/// default).
fn caption_size(node: &XfaNode) -> Option<(Side, f64, f64)> {
    let c = node.child("caption").filter(|c| occupies(c))?;
    let side = side(c)?;
    let natural = plain_text(c)
        .filter(|_| !has_unplain_value(c))
        .map_or((0.0, 0.0), |t| {
            text_extent(t, c.child("font"), c.child("para"), 0.0, false)
        });
    let m = inset(LayoutRect::default(), c.child("margin"));
    let (mut w, mut h) = (natural.0 - m.w, natural.1 - m.h);
    if let Some(r) = length(c, "reserve").filter(|v| *v > 0.0) {
        match side {
            Side::Left | Side::Right => w = r,
            Side::Top | Side::Bottom => h = r,
        }
    }
    Some((side, w, h))
}

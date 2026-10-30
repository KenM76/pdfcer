//! A `checkButton` widget: its box and, when the field is on, its mark
//! (XFA 3.3 checkButton pp.650-651, pp.480-481; items pp.758-759;
//! exclGroup p.715; slice `adobe_ext__xfa_appearance_drawing.md` §8.1).
//!
//! Readings of gap G-D13: the box is always drawn (p.480 describes the
//! widget as a box), styled by the widget's own `border` when it has one and
//! by the default edge (0.5pt black, p.687) when it has none. Without an
//! `items` element a field is on when its value is `1` or `on` (p.481). A
//! neutral value (the third `items` entry) draws the box alone and is
//! reported. Mark geometry is pdfcer's own (p.651 leaves it open), shared
//! with AcroForm check boxes.

use crate::annot_author::{CheckStyle, draw_check_mark};
use crate::writer::content::{LineCap, LineJoin, Paint};

use super::{LayoutItem, LayoutRect, PaintNoteKind, Painter, XfaNode, length};

/// Whether `node` is a field drawn as a check box.
pub(super) fn is_check_button(node: &XfaNode) -> bool {
    super::text::widget(node).is_some_and(|w| w.tag == "checkButton")
}

/// The text of `holder`'s single value child (`text`, `integer`, ...).
fn value_of(holder: &XfaNode) -> Option<&str> {
    let v = holder.child("value")?.children.first()?;
    Some(v.text.trim())
}

/// The field's on, off and neutral values (p.759; missing ones are null).
fn states(node: &XfaNode) -> Option<[&str; 3]> {
    let items = node.child("items")?;
    let get = |i: usize| items.children.get(i).map_or("", |c| c.text.trim());
    Some([get(0), get(1), get(2)])
}

/// How the field's value reads: on, off, or the neutral state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    On,
    Off,
    Neutral,
}

/// The state of `node`'s own value.
fn state(node: &XfaNode) -> State {
    let Some(value) = value_of(node) else {
        return State::Off;
    };
    match states(node) {
        Some([on, ..]) if value == on => State::On,
        Some([_, _, neutral]) if !neutral.is_empty() && value == neutral => State::Neutral,
        Some(_) => State::Off,
        None if value == "1" || value.eq_ignore_ascii_case("on") => State::On,
        None => State::Off,
    }
}

/// The mark `mark` names on a box of `shape` (p.651).
fn style(mark: Option<&str>, round: bool) -> CheckStyle {
    match mark {
        Some("check") => CheckStyle::Check,
        Some("circle") => CheckStyle::Circle,
        Some("cross") => CheckStyle::Cross,
        Some("diamond") => CheckStyle::Diamond,
        Some("square") => CheckStyle::Square,
        Some("star") => CheckStyle::Star,
        _ if round => CheckStyle::Circle,
        _ => CheckStyle::Cross,
    }
}

impl Painter<'_> {
    /// Draw the check box of `node` in `r` (the region left after the
    /// caption and the widget's margin), aligned in it by the field's
    /// `para`, with its mark when `node` is on.
    pub(super) fn check_button(&mut self, item: &LayoutItem, node: &XfaNode, r: LayoutRect) {
        let Some(w) = super::text::widget(node) else {
            return;
        };
        let size = length(w, "size").filter(|s| *s > 0.0).unwrap_or(10.0);
        let size = size.min(r.w).min(r.h);
        if size <= 0.0 {
            return;
        }
        let para = node.child("para");
        let frac = |attr: &str, mid: &str, end: &str| match para.and_then(|p| p.attr(attr)) {
            Some(a) if a == mid => 0.5,
            Some(a) if a == end => 1.0,
            _ => 0.0,
        };
        let x = r.x + (r.w - size) * frac("hAlign", "center", "right");
        let y = r.y + (r.h - size) * frac("vAlign", "middle", "bottom");
        let bx = LayoutRect {
            x,
            y,
            w: size,
            h: size,
        };
        let round = w.attr("shape") == Some("round");
        self.check_box(item, w, bx, round);
        match state(node) {
            State::On => self.check_mark(item, node, w, bx, round),
            State::Neutral => self.note(item, PaintNoteKind::Content),
            State::Off => {}
        }
    }

    /// The box: the widget's `border` when present (a round box takes its
    /// first edge as the circle's stroke), else the default edge.
    fn check_box(&mut self, item: &LayoutItem, w: &XfaNode, bx: LayoutRect, round: bool) {
        let border = w.child("border");
        if let Some(b) = border
            && !round
        {
            self.border(item, b, bx);
            return;
        }
        let edge = match border.and_then(|b| b.child("edge")) {
            Some(e) => self.edge(item, e),
            None => super::Edge {
                thickness: 0.5,
                rgb: [0.0; 3],
                drawn: true,
            },
        };
        if !edge.drawn {
            return;
        }
        self.stroke_style(edge, LineCap::Butt);
        let (cx, cy) = (bx.x + bx.w / 2.0, self.height - bx.y - bx.h / 2.0);
        if round {
            self.circle(cx, cy, bx.w / 2.0);
        } else {
            self.b.rect(bx.x, self.height - bx.y - bx.h, bx.w, bx.h);
        }
        self.b.paint(Paint::Stroke);
    }

    fn check_mark(
        &mut self,
        item: &LayoutItem,
        node: &XfaNode,
        w: &XfaNode,
        bx: LayoutRect,
        round: bool,
    ) {
        let rgb = match node.child("font").and_then(|f| f.child("fill")) {
            Some(fill) => self.color(item, fill, [0.0; 3]),
            None => [0.0; 3],
        };
        let s = bx.w / 4.0;
        let (cx, cy) = (bx.x + bx.w / 2.0, self.height - bx.y - bx.h / 2.0);
        self.b.save_state();
        self.b.set_stroke_rgb(rgb[0], rgb[1], rgb[2]);
        self.b.set_fill_rgb(rgb[0], rgb[1], rgb[2]);
        self.b.set_line_cap(LineCap::Round);
        self.b.set_line_join(LineJoin::Round);
        draw_check_mark(&mut self.b, style(w.attr("mark"), round), (cx, cy), s);
        self.b.restore_state();
    }

    /// A circle of radius `r` about `(cx, cy)` as four Bézier arcs.
    fn circle(&mut self, cx: f64, cy: f64, r: f64) {
        let k = r * 0.552_284_749_8;
        self.b.move_to(cx + r, cy);
        self.b.curve_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
        self.b.curve_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
        self.b.curve_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
        self.b.curve_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
        self.b.close_subpath();
    }
}

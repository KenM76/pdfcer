//! A comb field (p.657): its value one character per cell, tines between
//! the cells drawn the full height of the field in the colour and width of
//! the widget border's bottom edge. Cell geometry is unstated (gap G-W17):
//! here `N` equal cells across the widget's border box, each character
//! centred in its cell. Spec RAG slice `adobe_ext__xfa_widgets_paint` §8.2.

use super::text::{metrics, para_inset};
use super::{Edge, LayoutItem, LayoutRect, PaintNoteKind, Painter, Presence, XfaNode, inset};
use crate::vartext::encode_winansi;
use crate::writer::content::{LineCap, Paint};

/// The widgets a `comb` may sit in (p.657).
const COMBED: [&str; 3] = ["textEdit", "numericEdit", "dateTimeEdit"];

impl Painter<'_> {
    /// Paint `node`'s value as a comb across `bx` (the widget's border box)
    /// when its widget `w` has a `comb`; `false` when it has none.
    pub(super) fn comb(
        &mut self,
        item: &LayoutItem,
        node: &XfaNode,
        w: &XfaNode,
        bx: LayoutRect,
    ) -> bool {
        let Some(comb) = w.child("comb").filter(|_| COMBED.contains(&w.tag.as_str())) else {
            return false;
        };
        let n = cells(comb, node);
        if let Some(tine) = self.tine(item, w) {
            self.tines(bx, n, tine);
        }
        let clusters = self
            .shown(item, node)
            .map(|t| clusters(&t))
            .unwrap_or_default();
        let unencodable = clusters.iter().any(|c| encode_winansi(c).1 > 0);
        if clusters.len() > n || unencodable {
            self.note(item, PaintNoteKind::Text);
        }
        let (font, para) = (node.child("font"), node.child("para"));
        let mut style = self.style(item, font, para);
        style.h_align = 0.5;
        let text_box = para_inset(inset(bx, w.child("margin")), para);
        let (ascent, line_h) = metrics(style.font, style.size, para);
        let top = self.height - text_box.y - (text_box.h - line_h).max(0.0) * style.v_align;
        #[allow(clippy::cast_precision_loss)] // a cell count fits f64 exactly
        let cw = bx.w / n as f64;
        for (i, c) in clusters.iter().take(n).enumerate() {
            #[allow(clippy::cast_precision_loss)] // a cell index fits f64 exactly
            let x = bx.x + cw * i as f64;
            let cell = LayoutRect { x, w: cw, ..bx };
            self.run(cell, &style, std::slice::from_ref(c), top - ascent, line_h);
        }
        true
    }

    /// The edge the tines are drawn with: the widget border's third
    /// (bottom) edge, the last one standing in for missing ones (p.38);
    /// `None` when the border is absent or not visible.
    fn tine(&mut self, item: &LayoutItem, w: &XfaNode) -> Option<Edge> {
        let border = w
            .child("border")
            .filter(|b| b.presence() == Presence::Visible)?;
        let edges: Vec<_> = border.children_named("edge").collect();
        let edge = match edges.get(2).or(edges.last()) {
            Some(e) => self.edge(item, e),
            None => Edge::PLAIN,
        };
        edge.drawn.then_some(edge)
    }

    /// The `n - 1` interior tines across `bx`.
    fn tines(&mut self, bx: LayoutRect, n: usize, tine: Edge) {
        if n < 2 {
            return;
        }
        self.stroke_style(tine, LineCap::Butt);
        let (top, bottom) = (self.height - bx.y, self.height - bx.y - bx.h);
        #[allow(clippy::cast_precision_loss)] // a cell count fits f64 exactly
        let cw = bx.w / n as f64;
        for i in 1..n {
            #[allow(clippy::cast_precision_loss)] // a cell index fits f64 exactly
            let x = bx.x + cw * i as f64;
            self.b.move_to(x, top);
            self.b.line_to(x, bottom);
        }
        self.b.paint(Paint::Stroke);
    }
}

/// The cell count (p.657): `numberOfCells`, else the value `text`
/// element's `maxChars`, else one.
fn cells(comb: &XfaNode, node: &XfaNode) -> usize {
    let positive = |s: Option<&str>| {
        s.and_then(|s| s.trim().parse::<usize>().ok())
            .filter(|&n| n > 0)
    };
    positive(comb.attr("numberOfCells"))
        .or_else(|| {
            let text = node.child("value").and_then(|v| v.child("text"));
            positive(text.and_then(|t| t.attr("maxChars")))
        })
        .unwrap_or(1)
}

/// `text` split into one string per cell: a combining mark joins the
/// character before it rather than taking a cell (p.657).
fn clusters(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in text.chars() {
        match out.last_mut() {
            Some(last) if combining(c) => last.push(c),
            _ => out.push(c.to_string()),
        }
    }
    out
}

/// Whether `c` is in a Unicode combining-diacritical-marks block.
fn combining(c: char) -> bool {
    matches!(u32::from(c), 0x300..=0x36F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

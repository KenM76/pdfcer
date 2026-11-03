//! A `choiceList` field's value: the display text of its selection, or its
//! whole list (XFA 3.3 pp.482-485, pp.652-653; spec RAG slice
//! `adobe_ext__xfa_widgets_paint` §4).

use super::text::{metrics, para_inset, plain_text};
use super::{LayoutItem, LayoutRect, Medium, PaintNoteKind, Painter, XfaNode};
use crate::writer::content::Paint;

/// The fill behind a selected item of an open list.
const SELECTED: [f64; 3] = [0.6, 0.75, 0.95];
/// A drop-down button's face.
const BUTTON: [f64; 3] = [0.83; 3];
/// A drop-down button's widest, in points.
const BUTTON_MAX: f64 = 15.0;

/// `node`'s display and save item texts, positionally paired (p.484): the
/// `items` with `save="1"` holds values, the other display text; one list
/// serves as both.
fn items(node: &XfaNode) -> (Vec<&str>, Vec<&str>) {
    let lists: Vec<&XfaNode> = node.children.iter().filter(|c| c.tag == "items").collect();
    let save = lists.iter().copied().find(|l| l.attr("save") == Some("1"));
    let display = lists
        .iter()
        .copied()
        .find(|l| l.attr("save") != Some("1"))
        .or(save);
    (texts(display), texts(save.or(display)))
}

fn texts(list: Option<&XfaNode>) -> Vec<&str> {
    list.map_or_else(Vec::new, |l| {
        l.children.iter().map(|c| c.text.as_str()).collect()
    })
}

/// The display text of `value` among `items`, positionally (p.485).
pub(in crate::xfa) fn display_of<'a>(node: &'a XfaNode, value: &str) -> Option<&'a str> {
    let (display, save) = items(node);
    let i = save.iter().position(|s| *s == value)?;
    display.get(i).copied()
}

impl Painter<'_> {
    /// Draw a `choiceList` field's value in `r`, the region inside the
    /// widget's margin.
    pub(super) fn choice(&mut self, item: &LayoutItem, node: &XfaNode, w: &XfaNode, r: LayoutRect) {
        let value = plain_text(node).unwrap_or("");
        let (display, save) = items(node);
        let unresolved = display.is_empty() && node.child("bindItems").is_some();
        if matches!(w.attr("open"), Some("always" | "multiSelect")) {
            if unresolved {
                self.note(item, PaintNoteKind::Choice);
            }
            // A multiSelect value is its selections, space-separated (p.485).
            let chosen: Vec<&str> = if w.attr("open") == Some("multiSelect") {
                value.split(' ').collect()
            } else {
                vec![value]
            };
            let selected: Vec<bool> = (0..display.len())
                .map(|i| save.get(i).is_some_and(|s| chosen.contains(s)))
                .collect();
            return self.open_list(item, node, &display, &selected, r);
        }
        let r = match self.opts.medium {
            Medium::Screen => self.drop_button(r),
            Medium::Paper => r,
        };
        if value.is_empty() {
            return;
        }
        let shown = display_of(node, value);
        if shown.is_none() && (unresolved || w.attr("textEntry") != Some("1")) {
            self.note(item, PaintNoteKind::Choice);
        }
        let text = shown.unwrap_or(value).to_owned();
        self.set(item, &text, node.child("font"), node.child("para"), r);
    }

    /// Every item, one per line from the top, the selected ones on a
    /// highlight (p.483); items past the region's foot are clipped.
    fn open_list(
        &mut self,
        item: &LayoutItem,
        node: &XfaNode,
        labels: &[&str],
        selected: &[bool],
        r: LayoutRect,
    ) {
        let (font, para) = (node.child("font"), node.child("para"));
        let style = self.style(item, font, para);
        let r = para_inset(r, para);
        let (ascent, line_h) = metrics(style.font, style.size, para);
        let mut y = r.y;
        for (i, _) in labels.iter().enumerate() {
            if r.h > 0.0 && y + line_h > r.y + r.h + 0.01 {
                self.note(item, PaintNoteKind::Text);
                break;
            }
            if selected.get(i) == Some(&true) {
                self.fill(LayoutRect { y, h: line_h, ..r }, SELECTED);
            }
            y += line_h;
        }
        let lines: Vec<String> = labels.iter().map(|s| (*s).to_owned()).collect();
        let top = self.height - r.y;
        self.run(r, &style, &lines, top - ascent, line_h);
    }

    /// Draw the screen drop-down button at `r`'s right; the rest of `r`.
    fn drop_button(&mut self, r: LayoutRect) -> LayoutRect {
        let w = r.h.min(r.w / 2.0).min(BUTTON_MAX);
        if w <= 0.0 {
            return r;
        }
        let x = r.x + r.w - w;
        self.fill(LayoutRect { x, w, ..r }, BUTTON);
        let (cx, cy) = (x + w / 2.0, self.height - r.y - r.h / 2.0);
        let s = w / 4.0;
        self.b.set_fill_rgb(0.0, 0.0, 0.0);
        self.b.move_to(cx - s, cy + s / 2.0);
        self.b.line_to(cx + s, cy + s / 2.0);
        self.b.line_to(cx, cy - s / 2.0);
        self.b.close_subpath();
        self.b.paint(Paint::Fill);
        LayoutRect { w: r.w - w, ..r }
    }
}

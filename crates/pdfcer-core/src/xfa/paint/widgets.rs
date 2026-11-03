//! A field's value as its widget shows it — through the display picture
//! (`format/picture`, Ch.26) and `passwordEdit`'s mask (p.788) — and a
//! `button`, which shows only its caption (p.462). Spec RAG slice
//! `adobe_ext__xfa_widgets_paint` §3, §6, §9.

use super::text::{plain_text, scalar};
use super::{LayoutItem, PaintNoteKind, Painter, Presence, XfaNode, inset};
use crate::xfa::picture::{self, ValueKind};

impl Painter<'_> {
    /// The text `node`'s value shows, or `None` for nothing to show.
    pub(super) fn shown(&mut self, item: &LayoutItem, node: &XfaNode) -> Option<String> {
        let raw = plain_text(node).unwrap_or("");
        let pic = node
            .child("format")
            .and_then(|f| f.child("picture"))
            .map(|p| p.text.trim())
            .filter(|p| !p.is_empty());
        let mut text = raw.to_owned();
        if let Some(pic) = pic {
            let kind = scalar(node).map_or(ValueKind::Text, |c| ValueKind::of(&c.tag));
            let rounding = self.opts.picture_rounding;
            if let Some(s) = picture::format(pic, raw, kind, self.english(item), rounding) {
                if s.approximated {
                    self.note(item, PaintNoteKind::Picture);
                }
                text = s.text;
            }
        }
        if let Some(w) = super::text::widget(node).filter(|w| w.tag == "passwordEdit") {
            let mask = w
                .attr("passwordChar")
                .and_then(|s| s.chars().next())
                .unwrap_or('*');
            text = text.chars().map(|_| mask).collect();
        }
        (!text.is_empty()).then_some(text)
    }

    /// Whether the locale in force at `item` is English: the nearest
    /// `locale` along its template path (p.1150), absent or `ambient`
    /// counting as English.
    fn english(&self, item: &LayoutItem) -> bool {
        let mut node = self.root;
        let mut locale = node.attr("locale");
        for &i in &item.path {
            let Some(n) = node.children.get(i) else { break };
            node = n;
            locale = node.attr("locale").or(locale);
        }
        locale.is_none_or(|l| l == "ambient" || l.starts_with("en"))
    }

    /// A `button` field: the widget's border, then the caption over the
    /// whole content region unless it reserves less (p.462).
    pub(super) fn button(&mut self, item: &LayoutItem, node: &XfaNode, w: Option<&XfaNode>) {
        let r = inset(item.rect, node.child("margin"));
        if let Some(b) = w
            .and_then(|w| w.child("border"))
            .filter(|b| b.presence() == Presence::Visible)
        {
            self.border(item, b, r);
        }
        self.caption_leaving(item, node, r, Some(0.0));
    }
}

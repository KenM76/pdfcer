//! A `draw`'s or `field`'s plain `value/text`, set in a standard-14 face
//! (XFA 3.3 text p.863, value p.882, font pp.741-744, para pp.800-803,
//! box model pp.49-54; slice `adobe_ext__xfa_appearance_drawing.md` §6-§7).

use crate::fontdata::{Std14, std14_descriptor, std14_styled};
use crate::linebreak::greedy_pack;
use crate::vartext::{encode_winansi, text_width};

use super::{FontWeight, LayoutItem, LayoutRect, PaintNoteKind, Painter, XfaNode, inset};

/// Widgets whose only drawing is the value (p.865, p.784, p.669, p.750).
const TEXT_WIDGETS: [&str; 5] = [
    "textEdit",
    "numericEdit",
    "dateTimeEdit",
    "passwordEdit",
    "imageEdit",
];

/// Whether `node` has drawable content this cut does not draw: a visible
/// caption, non-text value content, rich text, or a widget that draws more
/// than its text.
pub(super) fn content_left_out(node: &XfaNode) -> bool {
    let caption = super::caption::left_out(node);
    let value = has_unplain_value(node);
    if super::check::is_check_button(node) {
        return caption;
    }
    let ui = widget(node)
        .is_some_and(|w| !TEXT_WIDGETS.contains(&w.tag.as_str()) || w.child("border").is_some());
    caption || value || ui
}

/// Whether `holder`'s `value` holds anything but plain `text` or an `image`.
pub(super) fn has_unplain_value(holder: &XfaNode) -> bool {
    holder.child("value").is_some_and(|v| {
        v.children
            .iter()
            .any(|c| !(c.tag == "image" || (c.tag == "text" && c.children.is_empty())))
    })
}

/// The `ui` element's widget child (the one beside `extras` and `picture`).
pub(in crate::xfa) fn widget(node: &XfaNode) -> Option<&XfaNode> {
    node.child("ui")?
        .children
        .iter()
        .find(|c| c.tag != "extras" && c.tag != "picture")
}

/// The face `typeface` is drawn in, and whether that is a substitution (the
/// standard 14 carry no Arial, Myriad or the like; p.58 best match).
fn face(typeface: &str, bold: bool, italic: bool) -> (Std14, bool) {
    let t = typeface.trim().to_ascii_lowercase();
    let exact = matches!(
        t.as_str(),
        "courier" | "helvetica" | "times" | "times roman" | "times-roman"
    );
    let serif = [
        "times", "georgia", "garamond", "minion", "palatino", "cambria",
    ];
    let family = if t.contains("courier") || t.contains("mono") {
        Std14::Courier
    } else if serif.iter().any(|s| t.contains(s)) || (t.contains("serif") && !t.contains("sans")) {
        Std14::TimesRoman
    } else {
        Std14::Helvetica
    };
    let styled = std14_styled(family, bold, italic).unwrap_or(family);
    (styled, !exact)
}

/// The text to draw: `holder`'s `value/text` PCDATA, else nothing.
pub(super) fn plain_text(holder: &XfaNode) -> Option<&str> {
    let text = holder.child("value")?.child("text")?;
    (text.children.is_empty() && !text.text.is_empty()).then_some(text.text.as_str())
}

/// The face and size `font` sets text in, and whether the face is a
/// substitution; `weight` for a `font` naming none (gap G-T1).
fn face_of(font: Option<&XfaNode>, weight: FontWeight) -> (Std14, f64, bool) {
    let attr = |a| font.and_then(|f| f.attr(a));
    let bold = attr("weight").map_or(weight == FontWeight::Bold, |w| w == "bold");
    let italic = attr("posture") == Some("italic");
    let (face, substituted) = face(attr("typeface").unwrap_or("Courier"), bold, italic);
    let size = font
        .and_then(|f| super::length(f, "size"))
        .filter(|s| *s > 0.0)
        .unwrap_or(10.0);
    (face, size, substituted)
}

/// The distance between lines of text in `font`/`para`, as painting sets
/// them: layout splits a text container on this pitch (Ch.8 p.291). Every
/// standard-14 weight shares its family's line metrics, so the gap G-T1
/// weight setting cannot change it.
pub(in crate::xfa) fn line_pitch(font: Option<&XfaNode>, para: Option<&XfaNode>) -> f64 {
    let (face, size, _) = face_of(font, FontWeight::Normal);
    metrics(face, size, para).1
}

/// The ascent and the line height of `face` at `size` under `para`'s
/// `lineHeight`.
fn metrics(face: Std14, size: f64, para: Option<&XfaNode>) -> (f64, f64) {
    let d = std14_descriptor(face);
    let ascent = f64::from(d.ascender) / 1000.0 * size;
    let descent = f64::from(d.descender).abs() / 1000.0 * size;
    let line_h = para
        .and_then(|p| super::length(p, "lineHeight"))
        .unwrap_or(0.0)
        .max(ascent + descent);
    (ascent, line_h)
}

/// Text-run settings read from `font` and `para`.
struct Style {
    font: Std14,
    size: f64,
    rgb: [f64; 3],
    h_align: f64,
    v_align: f64,
}

impl Painter<'_> {
    /// Draw the item's caption (p.51) and value text or image: the caption in the
    /// region carved from the content region (p.50), the value in the rest,
    /// inset by a text widget's own margin.
    pub(super) fn text(&mut self, item: &LayoutItem, node: &XfaNode) {
        let mut r = self.value_region(item, node);
        if let Some(w) = widget(node) {
            r = inset(r, w.child("margin"));
        }
        if let Some(image) = super::image::value_image(node) {
            self.image(item, image, node.child("para"), r);
        } else if let Some(text) = plain_text(node) {
            self.set(item, text, node.child("font"), node.child("para"), r);
        }
    }

    /// The item's content region less its caption (p.50).
    pub(super) fn value_region(&mut self, item: &LayoutItem, node: &XfaNode) -> LayoutRect {
        let r = inset(item.rect, node.child("margin"));
        self.caption(item, node, r)
    }

    /// A `checkButton` field: its caption, then its box in the rest inset
    /// by the widget's margin.
    pub(super) fn check_field(&mut self, item: &LayoutItem, node: &XfaNode) {
        let r = inset(item.rect, node.child("margin"));
        let leave = (self.opts.check_caption == super::CheckCaption::Remainder)
            .then(|| super::check::box_size(node));
        let mut r = self.caption_leaving(item, node, r, leave);
        if let Some(w) = widget(node) {
            r = inset(r, w.child("margin"));
        }
        self.check_button(item, node, r);
    }

    /// The unwrapped width and height `text` takes in `font`/`para`.
    pub(super) fn extent(
        &mut self,
        item: &LayoutItem,
        text: &str,
        font: Option<&XfaNode>,
        para: Option<&XfaNode>,
    ) -> (f64, f64) {
        let style = self.style(item, font, para);
        let (lines, _) = self.lines(text, &style, 0.0);
        let (_, line_h) = metrics(style.font, style.size, para);
        let w = lines
            .iter()
            .map(|l| text_width(style.font, style.size, l))
            .fold(0.0, f64::max);
        #[allow(clippy::cast_precision_loss)] // a line count fits f64 exactly
        (w, line_h * lines.len() as f64)
    }

    /// Set `text` in `r` with `font`/`para`, wrapping at `r`'s width and
    /// clipped to `r` when it has area.
    pub(super) fn set(
        &mut self,
        item: &LayoutItem,
        text: &str,
        font: Option<&XfaNode>,
        para: Option<&XfaNode>,
        r: LayoutRect,
    ) {
        let style = self.style(item, font, para);
        let (lines, approximated) = self.lines(text, &style, r.w);
        if approximated {
            self.note(item, PaintNoteKind::Text);
        }
        let (ascent, line_h) = metrics(style.font, style.size, para);
        #[allow(clippy::cast_precision_loss)] // a line count fits f64 exactly
        let block_h = line_h * lines.len() as f64;
        if r.h > 0.0 && block_h > r.h + 0.01 {
            self.note(item, PaintNoteKind::Text);
        }
        let top = self.height - r.y - (r.h - block_h).max(0.0) * style.v_align;
        self.run(r, &style, &lines, top - ascent, line_h);
    }

    fn style(
        &mut self,
        item: &LayoutItem,
        font: Option<&XfaNode>,
        para: Option<&XfaNode>,
    ) -> Style {
        let (face, size, substituted) = face_of(font, self.opts.font_weight);
        if substituted {
            self.note(item, PaintNoteKind::Font);
        }
        let rgb = match font.and_then(|f| f.child("fill")) {
            Some(fill) => self.color(item, fill, [0.0; 3]),
            None => [0.0; 3],
        };
        let h_align = match para.and_then(|p| p.attr("hAlign")) {
            Some("center") => 0.5,
            Some("right") => 1.0,
            Some("justify" | "justifyAll" | "radix") => {
                self.note(item, PaintNoteKind::Text);
                0.0
            }
            _ => 0.0,
        };
        let v_align = match para.and_then(|p| p.attr("vAlign")) {
            Some("middle") => 0.5,
            Some("bottom") => 1.0,
            _ => 0.0,
        };
        Style {
            font: face,
            size,
            rgb,
            h_align,
            v_align,
        }
    }

    /// `text` broken at newlines and, when `width` is positive, wrapped at
    /// spaces; `true` when a character was not encodable or a word is wider
    /// than the region (drawn unbroken and clipped, not split, here).
    fn lines(&self, text: &str, style: &Style, width: f64) -> (Vec<String>, bool) {
        let mut out = Vec::new();
        let mut approximated = encode_winansi(text).1 > 0;
        let measure = |s: &str| text_width(style.font, style.size, s);
        for para in text.lines() {
            let words: Vec<&str> = para.split(' ').collect();
            if width <= 0.0 {
                out.push(para.to_owned());
                continue;
            }
            for range in greedy_pack(words.len(), width, |a, b| {
                words.get(a..b).map_or(0.0, |w| measure(&w.join(" ")))
            }) {
                let line = words.get(range).unwrap_or_default().join(" ");
                approximated |= measure(&line) > width + 0.01;
                out.push(line);
            }
        }
        (out, approximated)
    }

    fn run(&mut self, r: LayoutRect, style: &Style, lines: &[String], first: f64, line_h: f64) {
        let clip = r.w > 0.0 && r.h > 0.0;
        self.b.save_state();
        if clip {
            self.b.rect(r.x, self.height - r.y - r.h, r.w, r.h);
            self.b.clip_nonzero();
            self.b.paint(crate::writer::content::Paint::NoPaint);
        }
        let name = self.font_name(style.font);
        self.b
            .set_fill_rgb(style.rgb[0], style.rgb[1], style.rgb[2]);
        self.b.begin_text();
        self.b.set_font(name.as_bytes(), style.size);
        let mut y = first;
        for line in lines {
            let x = r.x + (r.w - text_width(style.font, style.size, line)).max(0.0) * style.h_align;
            self.b.set_text_matrix(1.0, 0.0, 0.0, 1.0, x, y);
            self.b.show_text(&encode_winansi(line).0);
            y -= line_h;
        }
        self.b.end_text();
        self.b.restore_state();
    }

    /// The page-resource name for `font`, registering it on first use.
    fn font_name(&mut self, font: Std14) -> String {
        let i = self
            .fonts
            .iter()
            .position(|f| *f == font)
            .unwrap_or_else(|| {
                self.fonts.push(font);
                self.fonts.len() - 1
            });
        format!("XF{i}")
    }
}

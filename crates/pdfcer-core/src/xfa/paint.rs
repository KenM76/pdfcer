//! Drawing a laid-out XFA page as PDF content (Pass 647.4; decision 204).
//!
//! [`paint_page`] turns one [`LayoutPage`] into an unfiltered content stream
//! in the page's default user space (origin bottom-left, points). It writes
//! nothing to the document: the caller draws it in place of the placeholder
//! page and discloses [`PaintedPage::notes`] off-canvas (rule 4).
//!
//! Spec: XFA 3.3 border p.629, edge p.687, corner p.663, fill p.734, color
//! p.655, presence p.68, box model pp.49-50, Z-order p.70; slice
//! `adobe_ext__xfa_appearance_drawing.md` (gaps G-D1..G-D19).
//!
//! Scope: a container's own `border` (its `fill`, `margin` and solid square
//! edges), and a `draw`'s or `field`'s plain value text in a standard-14
//! face ([`PaintedPage::resources`] names them), beside its left, right, top
//! or bottom caption, and a `checkButton` field's box and mark. Inline and
//! rich captions, rich text, other value content and other non-text widgets
//! are not drawn yet; each item carrying them is reported as
//! [`PaintNoteKind::Content`].

use crate::fontdata::Std14;
use crate::object::{Dict, Name, Object};
use crate::vartext::standard14_font_dict;
use crate::writer::content::{ContentBuilder, LineCap, Paint};

use super::{ContainerKind, LayoutItem, LayoutPage, LayoutRect, Presence, Template, XfaNode};

/// Where an `edge` with `hand="even"` sits (gap G-D1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum EdgePlacement {
    /// Centred on the border rectangle, as the grammar's `hand` default
    /// says (p.630).
    #[default]
    Grammar,
    /// Grown inward so the border stays inside its extent, as Ch.2 p.38
    /// says (the same as `hand="right"`).
    Inward,
}

/// What a `border` with no `fill` element paints (gap G-D2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MissingFill {
    /// Nothing, as the `fill` grammar says (p.734).
    #[default]
    None,
    /// Solid white, as the Ch.2 p.39 note says.
    White,
}

/// The weight of a `font` with no `weight` attribute (gap G-T1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum FontWeight {
    /// `normal`, as the `font` grammar says (p.741).
    #[default]
    Normal,
    /// `bold`, as the Ch.2 p.58 prose says.
    Bold,
}

/// The font a `caption` with no `font` of its own is set in (gap G-D11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum CaptionFont {
    /// The `font` defaults (Courier 10pt), as the grammar gives an omitted
    /// `font` (p.643, p.741).
    #[default]
    Own,
    /// The enclosing field's or draw's `font`, the inheritance p.43 allows.
    Field,
}

/// The settings [`paint_page`] draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct PaintOptions {
    /// Gap G-D1.
    pub edge_placement: EdgePlacement,
    /// Gap G-D2.
    pub missing_fill: MissingFill,
    /// Gap G-T1.
    pub font_weight: FontWeight,
    /// Gap G-D11.
    pub caption_font: CaptionFont,
}

impl PaintOptions {
    /// The spec-grammar defaults.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            edge_placement: EdgePlacement::Grammar,
            missing_fill: MissingFill::None,
            font_weight: FontWeight::Normal,
            caption_font: CaptionFont::Own,
        }
    }

    /// With `edge_placement`.
    #[must_use]
    pub const fn with_edge_placement(mut self, edge_placement: EdgePlacement) -> Self {
        self.edge_placement = edge_placement;
        self
    }

    /// With `missing_fill`.
    #[must_use]
    pub const fn with_missing_fill(mut self, missing_fill: MissingFill) -> Self {
        self.missing_fill = missing_fill;
        self
    }

    /// With `font_weight`.
    #[must_use]
    pub const fn with_font_weight(mut self, font_weight: FontWeight) -> Self {
        self.font_weight = font_weight;
        self
    }

    /// With `caption_font`.
    #[must_use]
    pub const fn with_caption_font(mut self, caption_font: CaptionFont) -> Self {
        self.caption_font = caption_font;
        self
    }
}

/// What [`paint_page`] drew differently from the template, or left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PaintNoteKind {
    /// An `inline` or non-plain-text caption, rich text, non-text value
    /// content or non-text widget: not drawn yet.
    Content,
    /// A non-`solid` edge stroke (dashed, 3-D...): that edge is not drawn.
    Stroke,
    /// A `linear`, `radial`, `pattern` or `stipple` fill: not drawn.
    FillType,
    /// A rounded or inverted corner: drawn square.
    Corner,
    /// `break="open"`: the border is drawn closed on every fragment.
    OpenBreak,
    /// An unreadable `color` value: the enclosing object's default is used.
    Color,
    /// A typeface that is not a standard-14 face: drawn in the nearest one
    /// (Courier, Times or Helvetica family; p.58 best match).
    Font,
    /// Text drawn approximately: a character outside `WinAnsiEncoding` shown
    /// as `?`, a word wider than its region, `justify`/`radix` alignment
    /// drawn left, or lines taller than the region (clipped).
    Text,
}

/// One disclosure from [`paint_page`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PaintNote {
    /// The item's template path ([`LayoutItem::path`]).
    pub path: Vec<usize>,
    /// What happened.
    pub kind: PaintNoteKind,
}

/// One page's drawing.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct PaintedPage {
    /// Content-stream bytes (unfiltered), in default user space.
    pub content: Vec<u8>,
    /// The resource dictionary `content` needs: `/Font` with the standard-14
    /// faces it names (`/XF0`, `/XF1`, ...); empty when it shows no text.
    pub resources: Dict,
    /// What was approximated or left out, in paint order.
    pub notes: Vec<PaintNote>,
}

/// Draw `page`, laid out from `template`.
///
/// Page background items paint first, then the foreground, each in layout
/// order with the first sibling deepest (p.70); within an item the fill is
/// under the edges. An item that is not visible, or inside an ancestor that
/// is not, draws nothing (p.68).
#[must_use]
pub fn paint_page(template: &Template, page: &LayoutPage, opts: &PaintOptions) -> PaintedPage {
    let mut p = Painter {
        b: ContentBuilder::new(),
        notes: Vec::new(),
        height: page.height,
        opts: *opts,
        fonts: Vec::new(),
    };
    let (back, fore): (Vec<_>, Vec<_>) = page.items.iter().partition(|i| i.background);
    for item in back.into_iter().chain(fore) {
        if let Some(node) = drawable(template, item) {
            p.item(item, node);
        }
    }
    let resources = p.resources();
    PaintedPage {
        content: p.b.into_bytes(),
        notes: p.notes,
        resources,
    }
}

/// The item's template node, when it and every ancestor container draw.
fn drawable<'t>(template: &'t Template, item: &LayoutItem) -> Option<&'t XfaNode> {
    if !item.visible {
        return None;
    }
    let mut node = template.element();
    for &i in &item.path {
        node = node.children.get(i)?;
        if node.container_kind().is_some() && node.presence() != Presence::Visible {
            return None;
        }
    }
    Some(node)
}

struct Painter {
    b: ContentBuilder,
    notes: Vec<PaintNote>,
    height: f64,
    opts: PaintOptions,
    fonts: Vec<Std14>,
}

/// One side of a border, resolved.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Edge {
    thickness: f64,
    rgb: [f64; 3],
    drawn: bool,
}

impl Painter {
    fn note(&mut self, item: &LayoutItem, kind: PaintNoteKind) {
        let note = PaintNote {
            path: item.path.clone(),
            kind,
        };
        if !self.notes.contains(&note) {
            self.notes.push(note);
        }
    }

    fn item(&mut self, item: &LayoutItem, node: &XfaNode) {
        if !matches!(
            item.kind,
            ContainerKind::Field
                | ContainerKind::Draw
                | ContainerKind::Subform
                | ContainerKind::ExclGroup
        ) {
            return;
        }
        if let Some(border) = node.child("border")
            && border.presence() == Presence::Visible
        {
            self.border(item, border, item.rect);
        }
        if item.kind == ContainerKind::Field && check::is_check_button(node) {
            self.check_field(item, node);
        } else if matches!(item.kind, ContainerKind::Field | ContainerKind::Draw) {
            self.text(item, node);
        }
        if text::content_left_out(node) {
            self.note(item, PaintNoteKind::Content);
        }
    }

    fn resources(&self) -> Dict {
        let mut res = Dict::new();
        if self.fonts.is_empty() {
            return res;
        }
        let mut fonts = Dict::new();
        for (i, f) in self.fonts.iter().enumerate() {
            let name = format!("XF{i}");
            fonts.insert(
                Name::from(name.as_bytes()),
                Object::Dict(standard14_font_dict(*f)),
            );
        }
        res.insert(Name::from(b"Font"), Object::Dict(fonts));
        res
    }

    /// Paint `border` around `rect`, inset by the border's own margin.
    fn border(&mut self, item: &LayoutItem, border: &XfaNode, rect: LayoutRect) {
        let r = inset(rect, border.child("margin"));
        if border.attr("break").is_some_and(|b| b.starts_with("open")) {
            self.note(item, PaintNoteKind::OpenBreak);
        }
        self.fill(item, border, r);
        if border
            .children_named("corner")
            .any(|c| length(c, "radius").unwrap_or(0.0) > 0.0)
        {
            self.note(item, PaintNoteKind::Corner);
        }
        let edges: Vec<_> = border.children_named("edge").collect();
        let Some(&last) = edges.last() else {
            return;
        };
        let sides: Vec<Edge> = (0..4)
            .map(|i| self.edge(item, edges.get(i).copied().unwrap_or(last)))
            .collect();
        let offset = match (border.attr("hand"), self.opts.edge_placement) {
            (Some("left"), _) => 0.5,
            (Some("right"), _) | (_, EdgePlacement::Inward) => -0.5,
            _ => 0.0,
        };
        self.edges(r, &sides, offset);
    }

    fn fill(&mut self, item: &LayoutItem, border: &XfaNode, r: LayoutRect) {
        let rgb = match border.child("fill") {
            None => match self.opts.missing_fill {
                MissingFill::None => return,
                MissingFill::White => [1.0; 3],
            },
            Some(f) if f.presence() != Presence::Visible => return,
            Some(f) => {
                let other = ["linear", "radial", "pattern", "stipple"]
                    .iter()
                    .any(|t| f.child(t).is_some());
                if other {
                    self.note(item, PaintNoteKind::FillType);
                    return;
                }
                self.color(item, f, [1.0; 3])
            }
        };
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        self.b.set_fill_rgb(rgb[0], rgb[1], rgb[2]);
        let y = self.height - r.y - r.h;
        self.b.rect(r.x, y, r.w, r.h);
        self.b.paint(Paint::Fill);
    }

    fn edge(&mut self, item: &LayoutItem, e: &XfaNode) -> Edge {
        let solid = e.attr("stroke").is_none_or(|s| s == "solid");
        if !solid {
            self.note(item, PaintNoteKind::Stroke);
        }
        let thickness = length(e, "thickness").unwrap_or(0.5);
        Edge {
            thickness,
            rgb: self.color(item, e, [0.0; 3]),
            drawn: solid && e.presence() == Presence::Visible && thickness > 0.0,
        }
    }

    /// `node`'s `color` child, or `default` (p.655).
    fn color(&mut self, item: &LayoutItem, node: &XfaNode, default: [f64; 3]) -> [f64; 3] {
        let Some(v) = node.child("color").and_then(|c| c.attr("value")) else {
            return default;
        };
        parse_rgb(v).unwrap_or_else(|| {
            self.note(item, PaintNoteKind::Color);
            default
        })
    }

    /// Stroke the four sides (top, right, bottom, left; p.38) of `r`, each
    /// moved outward by `offset` of its thickness (`hand`, p.630).
    fn edges(&mut self, r: LayoutRect, sides: &[Edge], offset: f64) {
        let Some(&first) = sides.first() else {
            return;
        };
        let (x0, x1) = (r.x, r.x + r.w);
        let (top, bottom) = (self.height - r.y, self.height - r.y - r.h);
        if first.drawn && sides.iter().all(|s| *s == first) {
            let d = first.thickness * offset;
            self.stroke_style(first, LineCap::ProjectingSquare);
            self.b
                .rect(x0 - d, bottom - d, r.w + 2.0 * d, r.h + 2.0 * d);
            self.b.paint(Paint::Stroke);
            return;
        }
        for (i, s) in sides.iter().enumerate() {
            if !s.drawn {
                continue;
            }
            let d = s.thickness * offset;
            let (a, b) = match i {
                0 => ((x0, top + d), (x1, top + d)),
                1 => ((x1 + d, top), (x1 + d, bottom)),
                2 => ((x1, bottom - d), (x0, bottom - d)),
                _ => ((x0 - d, bottom), (x0 - d, top)),
            };
            self.stroke_style(*s, LineCap::ProjectingSquare);
            self.b.move_to(a.0, a.1);
            self.b.line_to(b.0, b.1);
            self.b.paint(Paint::Stroke);
        }
    }

    fn stroke_style(&mut self, e: Edge, cap: LineCap) {
        self.b.set_line_width(e.thickness);
        self.b.set_line_cap(cap);
        self.b.set_stroke_rgb(e.rgb[0], e.rgb[1], e.rgb[2]);
    }
}

/// `r` inset by a `margin` element's insets (p.776; default 0).
fn inset(r: LayoutRect, margin: Option<&XfaNode>) -> LayoutRect {
    let Some(m) = margin else {
        return r;
    };
    let get = |a| length(m, a).unwrap_or(0.0);
    let (l, t) = (get("leftInset"), get("topInset"));
    let (rt, b) = (get("rightInset"), get("bottomInset"));
    LayoutRect {
        x: r.x + l,
        y: r.y + t,
        w: r.w - l - rt,
        h: r.h - t - b,
    }
}

fn length(node: &XfaNode, attr: &str) -> Option<f64> {
    node.measurement(attr)?.points()
}

/// `"r,g,b"`, each 0-255 (p.655), as 0-1 components.
fn parse_rgb(v: &str) -> Option<[f64; 3]> {
    let mut out = [0.0; 3];
    let mut parts = v.split(',');
    for c in &mut out {
        let n: u16 = parts.next()?.trim().parse().ok()?;
        if n > 255 {
            return None;
        }
        *c = f64::from(n) / 255.0;
    }
    parts.next().is_none().then_some(out)
}

mod caption;
mod check;
mod text;

#[cfg(test)]
mod tests;

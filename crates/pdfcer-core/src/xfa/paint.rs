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
//! Scope: a container's own `border` (its `fill`, `margin`, corners and
//! solid, dashed or 3-D edges); a `draw`'s `rectangle`, `line` or `arc`
//! value; a `draw`'s or `field`'s plain or rich value text in standard-14
//! faces ([`PaintedPage::resources`] names them), beside its left, right,
//! top or bottom caption, itself plain or rich; a `checkButton` field's box and mark; an `image` value
//! or caption, inline or linked through [`XfaImages`]. A rotated draw or
//! field is drawn turned. A `fill` is solid, a `linear` or `radial`
//! shading ([`PaintedPage::resources`] names them), `pattern` hatching or a
//! `stipple` blend. Other value content and other non-text widgets are not
//! drawn yet; each item carrying them is reported as
//! [`PaintNoteKind::Content`].

use crate::fontdata::Std14;
use crate::object::{Dict, Name, Object};
use crate::vartext::standard14_font_dict;
use crate::writer::content::{ContentBuilder, LineCap, Paint};

pub use dash::{DashCaps, DashLengths};
pub use options::{
    CaptionFont, CheckCaption, CheckEdge, EdgePlacement, FontWeight, ImageOverflow, ImagePlacement,
    Medium, MissingFill, PaintOptions, PictureRounding, Stipple, ThreeDStroke,
};
pub use shape::{ArcCircle, ArcFill};

use super::{
    ContainerKind, LayoutItem, LayoutPage, LayoutRect, Presence, Template, XfaImages, XfaNode,
};
/// What [`paint_page`] drew differently from the template, or left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PaintNoteKind {
    /// A non-plain-text caption, an `inline` caption beside a non-text or
    /// rich value, non-text value content or a non-text widget: not drawn
    /// yet.
    Content,
    /// An edge `stroke` value the grammar does not name (p.688): drawn
    /// solid.
    Stroke,
    /// A 3-D edge stroke (`lowered`, `raised`, `etched`, `embossed`) drawn
    /// without the witness's bands: plain in its thickness and colour (by
    /// [`ThreeDStroke::Plain`], on a line draw, or on a later edge when the
    /// first edge, which sets a box's bevel, is not 3-D), or in two greys
    /// on a border with rounded corners or an arc.
    StrokeApproximated,
    /// Unused: every `fill` type is drawn.
    FillType,
    /// An inverted corner with a radius: its concave shape is pdfcer's
    /// reading of the template's "concave" (gap G-D18).
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
    /// drawn left, lines taller than the region (clipped), or a comb
    /// value with more characters than cells (the rest not drawn).
    Text,
    /// An `image` not drawn: its `href` names nothing in the file's
    /// `XFAImages` tree (it is never followed outside the file) and it has
    /// no usable inline content, or its bytes are not a JPEG, PNG, GIF, BMP
    /// or TIFF image.
    Image,
    /// An image drawn with its transparency composited onto white.
    ImageTransparency,
    /// A `line` `slope`, an `arc` angle or an edge `cap` value the grammar
    /// does not name or that is not a number: its default is used.
    Shape,
    /// A `rotate` that is not a multiple of 90 or not a number, or one on
    /// a subform or exclusion group, which do not rotate (p.55): handled
    /// per [`LayoutOptions::odd_rotate`](super::LayoutOptions::odd_rotate)
    /// or ignored.
    Rotate,
    /// A value shown from a recognised page-number script (`xfa.layout`
    /// page or count assigned on the `$layout` `ready` event), which is
    /// not run.
    Script,
    /// A `choiceList` value shown as saved: it matches no item and typing
    /// is not allowed, or the items come from `bindItems`, which are not
    /// resolved; an open list with `bindItems` shows no items.
    Choice,
    /// A value shown through its display picture (`format/picture`,
    /// Ch.26) only in part: a symbol pdfcer does not format left the value
    /// as saved, or month, day or currency names came out in English under
    /// another locale.
    Picture,
    /// A `barcode` field's value shown as text: barcodes are not encoded.
    Barcode,
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
    paint_page_with_images(template, page, opts, &XfaImages::new())
}

/// [`paint_page`], resolving each image `href` in `images` (only there).
#[must_use]
pub fn paint_page_with_images(
    template: &Template,
    page: &LayoutPage,
    opts: &PaintOptions,
    images: &XfaImages,
) -> PaintedPage {
    let mut p = Painter {
        images,
        root: template.element(),
        b: ContentBuilder::new(),
        notes: Vec::new(),
        height: page.height,
        opts: *opts,
        fonts: Vec::new(),
        dashed: false,
        shadings: Vec::new(),
        page: (page.number, page.count),
    };
    let (back, fore): (Vec<_>, Vec<_>) = page.items.iter().partition(|i| i.background);
    for item in back.into_iter().chain(fore) {
        if let Some(node) = drawable(template, item) {
            p.turned(item, node);
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

struct Painter<'i> {
    images: &'i XfaImages,
    /// The template root, where floating fields are looked up.
    root: &'i XfaNode,
    b: ContentBuilder,
    notes: Vec<PaintNote>,
    height: f64,
    opts: PaintOptions,
    fonts: Vec<Std14>,
    /// Whether the graphics state holds a dash pattern.
    dashed: bool,
    /// The `/Shading` resources, named `XS0`, `XS1`, ...
    shadings: Vec<Dict>,
    /// The page's number and the layout's page count.
    page: (usize, usize),
}

/// One side of a border, resolved.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Edge {
    thickness: f64,
    rgb: [f64; 3],
    drawn: bool,
    three_d: Option<bevel::ThreeD>,
    dash: Option<dash::Pattern>,
}

impl Edge {
    /// An omitted `edge`: 0.5pt solid black (p.687).
    const PLAIN: Self = Self {
        thickness: 0.5,
        rgb: [0.0; 3],
        drawn: true,
        three_d: None,
        dash: None,
    };
}

impl Painter<'_> {
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
            self.text_piece(item, node);
        }
        if text::content_left_out(node) {
            self.note(item, PaintNoteKind::Content);
        }
    }

    /// Text of `item`; a split piece sets its whole container's text and
    /// shows the lines inside the piece (Ch.8 p.291).
    fn text_piece(&mut self, item: &LayoutItem, node: &XfaNode) {
        let Some(f) = item.fragment else {
            return self.text(item, node);
        };
        let r = item.rect;
        let mut whole = item.clone();
        whole.rect.y = r.y - f.offset;
        whole.rect.h = f.height;
        self.b.save_state();
        self.b.rect(r.x, self.height - r.y - r.h, r.w, r.h);
        self.b.clip_nonzero();
        self.b.paint(crate::writer::content::Paint::NoPaint);
        self.text(&whole, node);
        self.b.restore_state();
    }

    fn resources(&self) -> Dict {
        let mut res = Dict::new();
        if !self.shadings.is_empty() {
            let mut sh = Dict::new();
            for (i, d) in self.shadings.iter().enumerate() {
                sh.insert(
                    Name::from(format!("XS{i}").as_bytes()),
                    Object::Dict(d.clone()),
                );
            }
            res.insert(Name::from(b"Shading"), Object::Dict(sh));
        }
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
        let fill = self
            .fill_of(item, border)
            .filter(|_| r.w > 0.0 && r.h > 0.0);
        let edges: Vec<_> = border.children_named("edge").collect();
        let sides: Vec<Edge> = match edges.last() {
            Some(&last) => (0..4)
                .map(|i| self.edge(item, edges.get(i).copied().unwrap_or(last)))
                .collect(),
            None => Vec::new(),
        };
        let offset = match (border.attr("hand"), self.opts.edge_placement) {
            (Some("left"), _) => 0.5,
            (Some("right"), _) | (_, EdgePlacement::Inward) => -0.5,
            _ => 0.0,
        };
        let rounded = corner::any_radius(border);
        if let Some(first) = sides.first().copied()
            && let Some(kind) = first.three_d.filter(|_| self.bevels() && !rounded)
        {
            if let Some(fill) = fill {
                self.fill_rect(r, fill);
            }
            if first.drawn {
                self.bevel_rect(r, kind, first.thickness);
            }
            return;
        }
        if sides.iter().any(|s| s.three_d.is_some() && s.drawn) {
            self.note(item, PaintNoteKind::StrokeApproximated);
        }
        if rounded {
            let corners = self.corners(item, border);
            match sides
                .first()
                .and_then(|f| f.three_d.filter(|_| f.drawn && self.bevels()))
            {
                Some(kind) => {
                    self.rounded(r, fill, &[], corners, offset);
                    let pad = sides.iter().map(|s| s.thickness).fold(0.0, f64::max);
                    self.two_tone(r, kind, pad, |p, rgb| {
                        let tone = |e: Edge| Edge {
                            rgb,
                            three_d: None,
                            ..e
                        };
                        let sides: Vec<Edge> = sides.iter().map(|&s| tone(s)).collect();
                        let corners = corners.map(|c| c.with_line(tone(c.line())));
                        p.rounded(r, None, &sides, corners, offset);
                    });
                }
                None => self.rounded(r, fill, &sides, corners, offset),
            }
            return;
        }
        if let Some(fill) = fill {
            self.fill_rect(r, fill);
        }
        self.edges(r, &sides, offset);
    }

    /// `fill` over `r`.
    fn fill_rect(&mut self, r: LayoutRect, fill: fill::Fill) {
        let y = self.height - r.y - r.h;
        self.paint_fill(fill, r, |p| p.b.rect(r.x, y, r.w, r.h));
    }

    fn fill(&mut self, r: LayoutRect, rgb: [f64; 3]) {
        self.b.set_fill_rgb(rgb[0], rgb[1], rgb[2]);
        let y = self.height - r.y - r.h;
        self.b.rect(r.x, y, r.w, r.h);
        self.b.paint(Paint::Fill);
    }

    fn edge(&mut self, item: &LayoutItem, e: &XfaNode) -> Edge {
        let stroke = e.attr("stroke");
        let three_d = stroke.and_then(bevel::ThreeD::from_stroke);
        let dash = stroke.and_then(dash::Pattern::from_stroke);
        let known = stroke.is_none_or(|s| s == "solid") || three_d.is_some() || dash.is_some();
        if !known {
            self.note(item, PaintNoteKind::Stroke);
        }
        let thickness = length(e, "thickness").unwrap_or(0.5);
        Edge {
            thickness,
            rgb: self.color(item, e, [0.0; 3]),
            drawn: e.presence() == Presence::Visible && thickness > 0.0,
            three_d,
            dash,
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

    fn bevels(&self) -> bool {
        self.opts.three_d_stroke == ThreeDStroke::Bevel
    }

    /// Width, cap and colour for stroking `e`; a dashed edge takes its
    /// dash's cap in place of `cap`.
    fn stroke_style(&mut self, e: Edge, cap: LineCap) {
        let cap = self.set_dash(e.dash, e.thickness, cap);
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

mod bevel;
mod caption;
mod check;
mod choice;
mod comb;
mod corner;
mod dash;
mod fill;
mod image;
mod options;
mod page_script;
mod rich;
mod rich_set;
mod shape;
mod size;
mod text;
mod turn;
mod widgets;
pub(super) use size::content_size;
pub(super) use text::{line_pitch, widget};

#[cfg(test)]
mod check_tests;
#[cfg(test)]
mod choice_tests;
#[cfg(test)]
mod rich_tests;
#[cfg(test)]
mod shape_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod widgets_tests;

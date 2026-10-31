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
//! value; a `draw`'s or `field`'s plain value text in a standard-14 face
//! ([`PaintedPage::resources`] names them), beside its left, right, top or
//! bottom caption; a `checkButton` field's box and mark; an `image` value
//! or caption, inline or linked through [`XfaImages`]. A rotated draw or
//! field is drawn turned. Inline and
//! rich captions, rich text, other value content and other non-text widgets
//! are not drawn yet; each item carrying them is reported as
//! [`PaintNoteKind::Content`].

use crate::fontdata::Std14;
use crate::object::{Dict, Name, Object};
use crate::vartext::standard14_font_dict;
use crate::writer::content::{ContentBuilder, LineCap, Paint};

pub use dash::{DashCaps, DashLengths};
pub use shape::{ArcCircle, ArcFill};

use super::{
    ContainerKind, LayoutItem, LayoutPage, LayoutRect, Presence, Template, XfaImages, XfaNode,
};

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

/// Where an image smaller than its region sits in it (gap G-D10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ImagePlacement {
    /// At the region's top-left corner.
    #[default]
    TopLeft,
    /// Where the holder's `para` `hAlign`/`vAlign` put it (p.800, p.803:
    /// defined for text only, so this reading is an extension).
    Para,
}

/// What happens to an image larger than its region (gap G-IM2: Adobe crops,
/// p.492; p.54 forbids clipping a printed field's content).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ImageOverflow {
    /// Clipped to the region.
    #[default]
    Crop,
    /// Drawn whole, past the region's edges.
    Show,
}

/// How a `lowered`, `raised`, `etched` or `embossed` edge is drawn (gap
/// G-CB11: p.688-689 state only the intent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ThreeDStroke {
    /// Grey bevel bands inside the border (the PDFium witness's table:
    /// fixed greys, the edge's colour ignored), box-wide from the first
    /// edge's stroke.
    #[default]
    Bevel,
    /// A plain stroke of the edge's thickness and colour, disclosed as
    /// [`PaintNoteKind::StrokeApproximated`].
    Plain,
}

/// The caption region of a `checkButton` with no positive `reserve` (gap
/// G-CB14; a negative reserve reads as omitted, G-CB12/G-CB13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum CheckCaption {
    /// Just enough for the caption content, as for any field (p.644).
    #[default]
    Fit,
    /// The whole widget less the box along the placement axis, so the box
    /// sits against the far edge (the PDFium witness).
    Remainder,
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
    /// Gap G-D10.
    pub image_placement: ImagePlacement,
    /// Gap G-IM2.
    pub image_overflow: ImageOverflow,
    /// The resolution assumed for an image that records none (p.51:
    /// application-dependent); 0 reads as 72.
    pub image_dpi: u16,
    /// Gap G-CB11.
    pub three_d_stroke: ThreeDStroke,
    /// Gap G-CB14.
    pub check_caption: CheckCaption,
    /// Gap G-D5.
    pub dash_lengths: DashLengths,
    /// The cap a dashed edge is drawn with.
    pub dash_caps: DashCaps,
    /// Gap G-D9, size.
    pub arc_circle: ArcCircle,
    /// Gap G-D9, closure.
    pub arc_fill: ArcFill,
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
            image_placement: ImagePlacement::TopLeft,
            image_overflow: ImageOverflow::Crop,
            image_dpi: 72,
            three_d_stroke: ThreeDStroke::Bevel,
            check_caption: CheckCaption::Fit,
            dash_lengths: DashLengths::Scaled,
            dash_caps: DashCaps::Projecting,
            arc_circle: ArcCircle::Diameter,
            arc_fill: ArcFill::Pie,
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

    /// With `image_placement`.
    #[must_use]
    pub const fn with_image_placement(mut self, image_placement: ImagePlacement) -> Self {
        self.image_placement = image_placement;
        self
    }

    /// With `image_overflow`.
    #[must_use]
    pub const fn with_image_overflow(mut self, image_overflow: ImageOverflow) -> Self {
        self.image_overflow = image_overflow;
        self
    }

    /// With `image_dpi`.
    #[must_use]
    pub const fn with_image_dpi(mut self, image_dpi: u16) -> Self {
        self.image_dpi = image_dpi;
        self
    }

    /// With `three_d_stroke`.
    #[must_use]
    pub const fn with_three_d_stroke(mut self, three_d_stroke: ThreeDStroke) -> Self {
        self.three_d_stroke = three_d_stroke;
        self
    }

    /// With `check_caption`.
    #[must_use]
    pub const fn with_check_caption(mut self, check_caption: CheckCaption) -> Self {
        self.check_caption = check_caption;
        self
    }

    /// With `dash_lengths`.
    #[must_use]
    pub const fn with_dash_lengths(mut self, dash_lengths: DashLengths) -> Self {
        self.dash_lengths = dash_lengths;
        self
    }

    /// With `dash_caps`.
    #[must_use]
    pub const fn with_dash_caps(mut self, dash_caps: DashCaps) -> Self {
        self.dash_caps = dash_caps;
        self
    }

    /// With `arc_circle`.
    #[must_use]
    pub const fn with_arc_circle(mut self, arc_circle: ArcCircle) -> Self {
        self.arc_circle = arc_circle;
        self
    }

    /// With `arc_fill`.
    #[must_use]
    pub const fn with_arc_fill(mut self, arc_fill: ArcFill) -> Self {
        self.arc_fill = arc_fill;
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
    /// An edge `stroke` value the grammar does not name (p.688): drawn
    /// solid.
    Stroke,
    /// A 3-D edge stroke (`lowered`, `raised`, `etched`, `embossed`) drawn
    /// as a plain stroke of its thickness and colour: by
    /// [`ThreeDStroke::Plain`], on a border with rounded corners, or on a
    /// later edge when the first edge (which sets a box's bevel) is not 3-D.
    StrokeApproximated,
    /// A `linear`, `radial`, `pattern` or `stipple` fill: not drawn.
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
    /// drawn left, or lines taller than the region (clipped).
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
        b: ContentBuilder::new(),
        notes: Vec::new(),
        height: page.height,
        opts: *opts,
        fonts: Vec::new(),
        dashed: false,
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
    b: ContentBuilder,
    notes: Vec<PaintNote>,
    height: f64,
    opts: PaintOptions,
    fonts: Vec<Std14>,
    /// Whether the graphics state holds a dash pattern.
    dashed: bool,
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
        let rgb = self
            .fill_rgb(item, border)
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
            if let Some(rgb) = rgb {
                self.fill(r, rgb);
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
            self.rounded(r, rgb, &sides, corners, offset);
            return;
        }
        if let Some(rgb) = rgb {
            self.fill(r, rgb);
        }
        self.edges(r, &sides, offset);
    }

    /// The colour `border`'s (or a shape's) fill paints, or `None` for no fill.
    fn fill_rgb(&mut self, item: &LayoutItem, border: &XfaNode) -> Option<[f64; 3]> {
        match border.child("fill") {
            None => match self.opts.missing_fill {
                MissingFill::None => None,
                MissingFill::White => Some([1.0; 3]),
            },
            Some(f) if f.presence() != Presence::Visible => None,
            Some(f) => {
                let other = ["linear", "radial", "pattern", "stipple"]
                    .iter()
                    .any(|t| f.child(t).is_some());
                if other {
                    self.note(item, PaintNoteKind::FillType);
                    return None;
                }
                Some(self.color(item, f, [1.0; 3]))
            }
        }
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
mod corner;
mod dash;
mod image;
mod shape;
mod size;
mod text;
mod turn;
pub(super) use size::content_size;
pub(super) use text::{line_pitch, widget};

#[cfg(test)]
mod check_tests;
#[cfg(test)]
mod shape_tests;
#[cfg(test)]
mod tests;

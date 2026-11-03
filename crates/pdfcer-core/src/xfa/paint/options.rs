//! The settings [`paint_page`](super::paint_page) draws with: one per reading the spec
//! leaves open, each defaulting to the viewer-matching one.

use super::dash::{DashCaps, DashLengths};
use super::shape::{ArcCircle, ArcFill};

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
    /// [`PaintNoteKind::StrokeApproximated`](super::PaintNoteKind::StrokeApproximated).
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

/// The outline of a square check box whose ui `border` has no `edge`
/// (gap G-CB4: p.38 strokes no edge; viewers draw the box).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum CheckEdge {
    /// The default edge (0.5pt black solid), as a round button draws and
    /// viewers draw a square one.
    #[default]
    Plain,
    /// None: the border is only filled (p.38).
    Spec,
}

/// Which presentation of the form is painted: widgets that differ between
/// screen and print follow it (a drop-down `choiceList`'s button,
/// pp.482-483; a `signature` widget, drawn on screen only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Medium {
    /// As a viewer shows it: drop-down buttons, signature borders.
    #[default]
    Screen,
    /// As it prints: neither.
    Paper,
}

/// The settings [`paint_page`](super::paint_page) draws with.
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
    /// Gap G-CB4, square box outline.
    pub check_edge: CheckEdge,
    /// Screen or print presentation.
    pub medium: Medium,
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
            check_edge: CheckEdge::Plain,
            medium: Medium::Screen,
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

    /// With `check_edge`.
    #[must_use]
    pub const fn with_check_edge(mut self, check_edge: CheckEdge) -> Self {
        self.check_edge = check_edge;
        self
    }

    /// With `medium`.
    #[must_use]
    pub const fn with_medium(mut self, medium: Medium) -> Self {
        self.medium = medium;
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

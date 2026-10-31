//! `xfa-layout --png-dir`: rasterize each computed XFA page to a PNG.

use super::*;

use pdfcer_core::xfa::{
    ArcCircle, ArcFill, CaptionFont, CheckCaption, DashCaps, DashLengths, EdgePlacement,
    FontWeight, ImageOverflow, ImagePlacement, MissingFill, PaintNoteKind, PaintOptions, Template,
    ThreeDStroke, XfaLayout,
};

/// `xfa-layout --edge-placement` (gap G-D1, decision 207).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum EdgePlacementArg {
    /// Centre an even-handed edge on the border line (the grammar).
    #[default]
    Grammar,
    /// Draw it inside the border line (the Ch.2 prose).
    Inward,
}

/// `xfa-layout --missing-fill` (gap G-D2, decision 207).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum MissingFillArg {
    /// A border with no fill element paints no fill (the grammar).
    #[default]
    None,
    /// It paints white (the p.39 note).
    White,
}

/// `xfa-layout --font-weight` (gap G-T1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum FontWeightArg {
    /// Normal weight, the XFA grammar's default.
    #[default]
    Normal,
    /// Bold, as the XFA specification's prose describes.
    Bold,
}

impl From<FontWeightArg> for FontWeight {
    fn from(w: FontWeightArg) -> Self {
        match w {
            FontWeightArg::Normal => Self::Normal,
            FontWeightArg::Bold => Self::Bold,
        }
    }
}

/// `xfa-layout --caption-font` (gap G-D11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum CaptionFontArg {
    /// The font defaults (the grammar).
    #[default]
    Own,
    /// The field's own font.
    Field,
}

/// `xfa-layout --image-placement` (gap G-D10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum ImagePlacementArg {
    /// At the box's top-left corner.
    #[default]
    TopLeft,
    /// Where the box's paragraph alignment puts it.
    Para,
}

/// `xfa-layout --image-overflow` (gap G-IM2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum ImageOverflowArg {
    /// Cut off at the box's edges.
    #[default]
    Crop,
    /// Drawn whole.
    Show,
}

/// `xfa-layout --three-d-stroke` (gap G-CB11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum ThreeDStrokeArg {
    /// Grey bevel bands.
    #[default]
    Bevel,
    /// A plain stroke of the edge's thickness and colour.
    Plain,
}

/// `xfa-layout --check-caption` (gap G-CB14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum CheckCaptionArg {
    /// Just wide enough for the caption.
    #[default]
    Fit,
    /// Everything but the box.
    Remainder,
}

/// `xfa-layout --dash-lengths` (gap G-D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum DashLengthsArg {
    /// Multiples of the edge's thickness.
    #[default]
    Scaled,
    /// The 3pt arrays Acrobat draws, at every thickness.
    Fixed,
}

/// `xfa-layout --dash-caps`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum DashCapsArg {
    /// Square ends projecting half the thickness.
    #[default]
    Projecting,
    /// Round dots and dash ends.
    Round,
}

/// `xfa-layout --arc-circle` (gap G-D9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum ArcCircleArg {
    /// The circle inscribed in the box.
    #[default]
    Diameter,
    /// A radius of the box's smaller side.
    Radius,
}

/// `xfa-layout --arc-fill` (gap G-D9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum ArcFillArg {
    /// Closed through the centre.
    #[default]
    Pie,
    /// Closed straight between the ends.
    Chord,
}

/// Where and how `xfa-layout` writes page images.
#[derive(Debug, Clone)]
pub(crate) struct XfaPngArgs {
    pub(crate) dir: PathBuf,
    pub(crate) scale: f32,
    pub(crate) edge_placement: EdgePlacementArg,
    pub(crate) missing_fill: MissingFillArg,
    pub(crate) font_weight: FontWeightArg,
    pub(crate) caption_font: CaptionFontArg,
    pub(crate) image_placement: ImagePlacementArg,
    pub(crate) image_overflow: ImageOverflowArg,
    pub(crate) image_dpi: u16,
    pub(crate) three_d_stroke: ThreeDStrokeArg,
    pub(crate) check_caption: CheckCaptionArg,
    pub(crate) dash_lengths: DashLengthsArg,
    pub(crate) dash_caps: DashCapsArg,
    pub(crate) arc_circle: ArcCircleArg,
    pub(crate) arc_fill: ArcFillArg,
}

impl XfaPngArgs {
    fn paint(&self) -> PaintOptions {
        PaintOptions::new()
            .with_edge_placement(match self.edge_placement {
                EdgePlacementArg::Grammar => EdgePlacement::Grammar,
                EdgePlacementArg::Inward => EdgePlacement::Inward,
            })
            .with_missing_fill(match self.missing_fill {
                MissingFillArg::None => MissingFill::None,
                MissingFillArg::White => MissingFill::White,
            })
            .with_font_weight(self.font_weight.into())
            .with_caption_font(match self.caption_font {
                CaptionFontArg::Own => CaptionFont::Own,
                CaptionFontArg::Field => CaptionFont::Field,
            })
            .with_image_placement(match self.image_placement {
                ImagePlacementArg::TopLeft => ImagePlacement::TopLeft,
                ImagePlacementArg::Para => ImagePlacement::Para,
            })
            .with_image_overflow(match self.image_overflow {
                ImageOverflowArg::Crop => ImageOverflow::Crop,
                ImageOverflowArg::Show => ImageOverflow::Show,
            })
            .with_image_dpi(self.image_dpi)
            .with_three_d_stroke(match self.three_d_stroke {
                ThreeDStrokeArg::Bevel => ThreeDStroke::Bevel,
                ThreeDStrokeArg::Plain => ThreeDStroke::Plain,
            })
            .with_check_caption(match self.check_caption {
                CheckCaptionArg::Fit => CheckCaption::Fit,
                CheckCaptionArg::Remainder => CheckCaption::Remainder,
            })
            .with_dash_lengths(match self.dash_lengths {
                DashLengthsArg::Scaled => DashLengths::Scaled,
                DashLengthsArg::Fixed => DashLengths::Fixed,
            })
            .with_dash_caps(match self.dash_caps {
                DashCapsArg::Projecting => DashCaps::Projecting,
                DashCapsArg::Round => DashCaps::Round,
            })
            .with_arc_circle(match self.arc_circle {
                ArcCircleArg::Diameter => ArcCircle::Diameter,
                ArcCircleArg::Radius => ArcCircle::Radius,
            })
            .with_arc_fill(match self.arc_fill {
                ArcFillArg::Pie => ArcFill::Pie,
                ArcFillArg::Chord => ArcFill::Chord,
            })
    }
}

/// Write `page-NNN.png` per page into `args.dir` and print one `rendered`
/// line per page and one `paint-note` line per thing not drawn (rule 4).
pub(crate) fn write_xfa_pngs(
    doc: &Document,
    template: &Template,
    layout: &XfaLayout,
    args: &XfaPngArgs,
) -> Result<(), String> {
    std::fs::create_dir_all(&args.dir)
        .map_err(|e| format!("cannot create {}: {e}", args.dir.display()))?;
    let paint = args.paint();
    let options = pdfcer_render::RenderOptions::default();
    let mut notes = 0usize;
    let view = doc.view();
    let images = pdfcer_core::xfa::XfaImages::from_view(&view);
    for (i, page) in layout.pages.iter().enumerate() {
        let out = pdfcer_render::xfa::render_xfa_page_with_images(
            &view, template, page, &paint, &images, args.scale, &options,
        )
        .map_err(|e| format!("page {i}: {e}"))?;
        let png = out
            .page
            .pixmap
            .encode_png()
            .map_err(|e| format!("page {i}: cannot encode PNG: {e}"))?;
        let file = args.dir.join(format!("page-{i:03}.png"));
        std::fs::write(&file, png).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
        println!(
            "rendered page={i} file={} width={} height={}",
            file.display(),
            out.page.pixmap.width(),
            out.page.pixmap.height()
        );
        for n in &out.notes {
            println!(
                "paint-note page={i} kind={} path={}",
                note_label(n.kind),
                dotted(&n.path)
            );
        }
        notes += out.notes.len();
    }
    println!("xfa-render pages={} notes={notes}", layout.pages.len());
    Ok(())
}

fn note_label(k: PaintNoteKind) -> &'static str {
    match k {
        PaintNoteKind::Content => "content-not-drawn",
        PaintNoteKind::Stroke => "stroke-drawn-solid",
        PaintNoteKind::StrokeApproximated => "stroke-approximated",
        PaintNoteKind::FillType => "fill-not-drawn",
        PaintNoteKind::Corner => "corner-inverted-inferred",
        PaintNoteKind::OpenBreak => "open-break-drawn-closed",
        PaintNoteKind::Color => "colour-defaulted",
        PaintNoteKind::Font => "font-substituted",
        PaintNoteKind::Text => "text-approximated",
        PaintNoteKind::Image => "image-not-drawn",
        PaintNoteKind::ImageTransparency => "image-transparency-flattened",
        PaintNoteKind::Shape => "shape-defaulted",
        PaintNoteKind::Rotate => "rotate-adjusted",
        _ => "other",
    }
}

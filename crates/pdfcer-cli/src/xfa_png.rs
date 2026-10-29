//! `xfa-layout --png-dir`: rasterize each computed XFA page to a PNG.

use super::*;

use pdfcer_core::xfa::{
    EdgePlacement, FontWeight, MissingFill, PaintNoteKind, PaintOptions, Template, XfaLayout,
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
    /// `normal` (the grammar).
    #[default]
    Normal,
    /// `bold` (the Ch.2 prose).
    Bold,
}

/// Where and how `xfa-layout` writes page images.
#[derive(Debug, Clone)]
pub(crate) struct XfaPngArgs {
    pub(crate) dir: PathBuf,
    pub(crate) scale: f32,
    pub(crate) edge_placement: EdgePlacementArg,
    pub(crate) missing_fill: MissingFillArg,
    pub(crate) font_weight: FontWeightArg,
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
            .with_font_weight(match self.font_weight {
                FontWeightArg::Normal => FontWeight::Normal,
                FontWeightArg::Bold => FontWeight::Bold,
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
    for (i, page) in layout.pages.iter().enumerate() {
        let out = pdfcer_render::xfa::render_xfa_page(
            &doc.view(),
            template,
            page,
            &paint,
            args.scale,
            &options,
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
        PaintNoteKind::Stroke => "stroke-not-drawn",
        PaintNoteKind::FillType => "fill-not-drawn",
        PaintNoteKind::Corner => "corner-drawn-square",
        PaintNoteKind::OpenBreak => "open-break-drawn-closed",
        PaintNoteKind::Color => "colour-defaulted",
        PaintNoteKind::Font => "font-substituted",
        PaintNoteKind::Text => "text-approximated",
        _ => "other",
    }
}

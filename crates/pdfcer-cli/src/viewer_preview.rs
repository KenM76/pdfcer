//! The viewer-preview flags `render-page` and `export-image` share:
//! `--no-overprint-preview`, `--separation-preview`, `--hide-ink`,
//! `--markup-without-ap`, `--free-text-c` and `--widgets-without-ap`.

use pdfcer_render::{
    Diagnostics, FreeTextC, MarkupWithoutAp, OverprintPreview, RenderOptions, SeparationPreview,
    WidgetsWithoutAp,
};

/// `--markup-without-ap`: what to show for a markup annotation (Ink,
/// Highlight, Underline, StrikeOut, Squiggly, Square, Circle, Polygon,
/// PolyLine, Line, FreeText) whose file stores no `/AP`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum MarkupWithoutApArg {
    /// Draw it from its own points and colours. A measurement line, a
    /// missing colour or bad geometry stays blank.
    #[default]
    FromGeometry,
    /// Leave it blank, as the file stores no drawing.
    Blank,
}

/// `--free-text-c`: what a FreeText's colour (`/C`) paints when the file
/// stores no `/AP` and no interior colour (`/IC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum FreeTextCArg {
    /// The colour fills the box, as Acrobat reads it.
    #[default]
    Fill,
    /// The colour draws the border, as pdfcer writes its own text boxes.
    Border,
}

/// `--widgets-without-ap`: what to show for a form field widget whose
/// file stores no `/AP`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum WidgetsWithoutApArg {
    /// Draw it from its value and field settings.
    #[default]
    Draw,
    /// Leave it blank.
    Blank,
}

/// The preview a render was asked for, resolved from its flags.
#[derive(Debug, Clone, Default)]
pub(crate) struct ViewerPreview {
    overprint: OverprintPreview,
    separation: Option<SeparationPreview>,
    markup: MarkupWithoutAp,
    free_text_c: FreeTextC,
    widgets: WidgetsWithoutAp,
}

impl ViewerPreview {
    /// Resolve the flags, saying on stderr when overprint preview is off:
    /// the overprint note's affected-paint count is then `0` by request,
    /// not because the page has nothing to overprint. `--hide-ink` implies
    /// `--separation-preview`.
    pub(crate) fn from_flags(
        no_overprint_preview: bool,
        separation_preview: bool,
        hide_ink: Vec<String>,
    ) -> Self {
        let overprint = if no_overprint_preview {
            eprintln!(
                "pdfcer: note: --no-overprint-preview: overprinting marks are drawn as \
                 knockouts, so the output shows the page as a viewer with Overprint Preview \
                 off does, not as it prints"
            );
            OverprintPreview::Off
        } else {
            OverprintPreview::Simulate
        };
        let separation = (separation_preview || !hide_ink.is_empty())
            .then(|| SeparationPreview::new().with_hidden(hide_ink));
        Self {
            overprint,
            separation,
            ..Self::default()
        }
    }

    /// This preview with the no-`/AP` drawing choices applied.
    pub(crate) fn with_no_ap(
        mut self,
        markup: MarkupWithoutApArg,
        widgets: WidgetsWithoutApArg,
    ) -> Self {
        self.markup = match markup {
            MarkupWithoutApArg::FromGeometry => MarkupWithoutAp::FromGeometry,
            MarkupWithoutApArg::Blank => MarkupWithoutAp::Blank,
        };
        self.widgets = match widgets {
            WidgetsWithoutApArg::Draw => WidgetsWithoutAp::Draw,
            WidgetsWithoutApArg::Blank => WidgetsWithoutAp::Blank,
        };
        self
    }

    /// This preview with `--free-text-c` applied.
    pub(crate) fn with_free_text_c(mut self, arg: FreeTextCArg) -> Self {
        self.free_text_c = match arg {
            FreeTextCArg::Fill => FreeTextC::Fill,
            FreeTextCArg::Border => FreeTextC::Border,
        };
        self
    }

    /// `options` with this preview applied.
    pub(crate) fn apply(self, options: RenderOptions) -> RenderOptions {
        options
            .with_overprint_preview(self.overprint)
            .with_separation_preview(self.separation)
            .with_markup_without_ap(self.markup)
            .with_free_text_c(self.free_text_c)
            .with_widgets_without_ap(self.widgets)
    }
}

/// The separation-preview note: the page's plates, what was hidden, what
/// matched nothing, and whether an additive page was composited in ink.
/// Silent for a render without a separation preview.
pub(crate) fn report_separation_preview(d: &Diagnostics) {
    if d.inks_forced_subtractive > 0 {
        eprintln!(
            "pdfcer: note: separation preview: this page's blending space is additive, so it \
             was composited in ink as if its page group declared DeviceCMYK; blends and \
             transparency can differ from a normal render"
        );
    }
    if !d.inks.is_empty() {
        eprintln!(
            "pdfcer: note: separation preview: inks on this page: {}; {} plate(s) hidden",
            d.inks.join(", "),
            d.inks_hidden
        );
    }
    if !d.inks_unmatched.is_empty() {
        eprintln!(
            "pdfcer: note: --hide-ink named no plate on this page: {} (names match exactly, \
             as listed above)",
            d.inks_unmatched.join(", ")
        );
    }
    if d.cmyk_spots_flattened > 0 && !d.inks.is_empty() {
        eprintln!(
            "pdfcer: note: separation preview: {} spot colorant(s) beyond the plane limit were \
             folded into process ink and cannot be hidden on their own",
            d.cmyk_spots_flattened
        );
    }
}

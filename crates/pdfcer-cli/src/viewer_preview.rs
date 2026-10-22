//! The viewer-preview flags `render-page` and `export-image` share:
//! `--no-overprint-preview`, `--separation-preview` and `--hide-ink`.

use pdfcer_render::{Diagnostics, OverprintPreview, RenderOptions, SeparationPreview};

/// The preview a render was asked for, resolved from its flags.
#[derive(Debug, Clone, Default)]
pub(crate) struct ViewerPreview {
    overprint: OverprintPreview,
    separation: Option<SeparationPreview>,
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
        }
    }

    /// `options` with this preview applied.
    pub(crate) fn apply(self, options: RenderOptions) -> RenderOptions {
        options
            .with_overprint_preview(self.overprint)
            .with_separation_preview(self.separation)
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

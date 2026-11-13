//! The viewer-preview flags `render-page` and `export-image` share:
//! `--no-overprint-preview`, `--separation-preview`, `--hide-ink`,
//! `--markup-without-ap`, `--free-text-c`, `--widgets-without-ap`,
//! `--soft-proof` (with `--simulate-paper`, `--simulate-black-ink`),
//! `--need-appearances`, `--widget-state`, `--edge-antialiasing` and `--preserve-calibrated-cmyk`.

use std::path::Path;

use pdfcer_core::annot::AppearanceKind;
use pdfcer_render::{
    Diagnostics, EdgeAntialiasing, FreeTextC, MarkupWithoutAp, NeedAppearances, OverprintPreview,
    RenderOptions, SeparationPreview, SoftProof, WidgetsWithoutAp,
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

impl From<FreeTextCArg> for FreeTextC {
    fn from(arg: FreeTextCArg) -> Self {
        match arg {
            FreeTextCArg::Fill => FreeTextC::Fill,
            FreeTextCArg::Border => FreeTextC::Border,
        }
    }
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

/// `--need-appearances`: what to show for a text or choice field when the
/// form sets `/NeedAppearances true` (its stored appearances may be stale).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum NeedAppearancesArg {
    /// Draw the field from its value; buttons and signatures keep their
    /// stored drawing. Nothing is written to the file.
    #[default]
    Honour,
    /// Show the stored drawing, as the file has it.
    AsStored,
}

/// `--widget-state`: which of a form field's three drawings to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum WidgetStateArg {
    /// The drawing shown when the pointer is elsewhere.
    #[default]
    Normal,
    /// The drawing shown while the pointer is over the field.
    Rollover,
    /// The drawing shown while the mouse button is held down on it.
    Down,
}

/// The preview a render was asked for, resolved from its flags.
#[derive(Debug, Clone, Default)]
pub(crate) struct ViewerPreview {
    overprint: OverprintPreview,
    separation: Option<SeparationPreview>,
    markup: MarkupWithoutAp,
    /// `None`: the settings file's `free_text_c`, already on the options.
    free_text_c: Option<FreeTextC>,
    widgets: WidgetsWithoutAp,
    need_appearances: NeedAppearances,
    widget_state: AppearanceKind,
    edges: EdgeAntialiasing,
    preserve_cmyk: bool,
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

    /// This preview proofed against the CMYK output profile at `path`
    /// (`--soft-proof`), which implies a separation preview. `Err` is the
    /// exit code, already reported.
    pub(crate) fn with_soft_proof(
        mut self,
        path: Option<&Path>,
        paper: bool,
        black_ink: bool,
    ) -> Result<Self, u8> {
        let Some(path) = path else {
            return Ok(self);
        };
        let bytes = match read_capped(path) {
            Ok(bytes) => bytes,
            Err(message) => {
                eprintln!("pdfcer: --soft-proof {}: {message}", path.display());
                return Err(crate::exit::RUNTIME_ERROR);
            }
        };
        let proof = match SoftProof::from_bytes(bytes) {
            Ok(proof) => proof.with_paper_colour(paper).with_black_ink(black_ink),
            Err(err) => {
                eprintln!("pdfcer: --soft-proof {}: {err}", path.display());
                return Err(crate::exit::EDIT_REFUSED);
            }
        };
        if !proof.simulates_black_ink() && !proof.compensates_black() {
            eprintln!(
                "pdfcer: note: --soft-proof: black point compensation cannot be applied to                  this profile, so its black shows as the profile measured it, as with                  --simulate-black-ink"
            );
        }
        let separation = self.separation.take().unwrap_or_default();
        self.separation = Some(separation.with_soft_proof(Some(proof)));
        Ok(self)
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

    /// This preview with `--need-appearances` applied.
    pub(crate) const fn with_need_appearances(mut self, arg: NeedAppearancesArg) -> Self {
        self.need_appearances = match arg {
            NeedAppearancesArg::Honour => NeedAppearances::Honour,
            NeedAppearancesArg::AsStored => NeedAppearances::AsStored,
        };
        self
    }

    /// This preview with `--widget-state` applied.
    pub(crate) const fn with_widget_state(mut self, arg: WidgetStateArg) -> Self {
        self.widget_state = match arg {
            WidgetStateArg::Normal => AppearanceKind::Normal,
            WidgetStateArg::Rollover => AppearanceKind::Rollover,
            WidgetStateArg::Down => AppearanceKind::Down,
        };
        self
    }

    /// This preview with `--free-text-c` applied, when it was given.
    pub(crate) fn with_free_text_c(mut self, arg: Option<FreeTextCArg>) -> Self {
        self.free_text_c = arg.map(Into::into);
        self
    }

    /// This preview with `--edge-antialiasing` resolved at `dpi`.
    pub(crate) fn with_edges(mut self, arg: crate::print_output::EdgeAaArg, dpi: f32) -> Self {
        // A DPI past u32 is far beyond the auto threshold either way.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let dpi = dpi.max(0.0).round() as u32;
        self.edges = arg.at_dpi(dpi);
        self
    }

    /// This preview with `--preserve-calibrated-cmyk` applied.
    pub(crate) const fn with_preserve_cmyk(mut self, on: bool) -> Self {
        self.preserve_cmyk = on;
        self
    }

    /// `options` with this preview applied.
    pub(crate) fn apply(self, options: RenderOptions) -> RenderOptions {
        // The flag turns the policy on for one run; it never turns off a
        // `settings.toml` that asked for it.
        let preserve = self.preserve_cmyk || options.policy().preserve_calibrated_cmyk;
        let options = options
            .with_overprint_preview(self.overprint)
            .with_separation_preview(self.separation)
            .with_markup_without_ap(self.markup)
            .with_widgets_without_ap(self.widgets)
            .with_need_appearances(self.need_appearances)
            .with_widget_state(self.widget_state)
            .with_edge_antialiasing(self.edges)
            .with_preserve_calibrated_cmyk(preserve);
        match self.free_text_c {
            Some(c) => options.with_free_text_c(c),
            None => options,
        }
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

/// The file at `path`, refused past the soft-proof profile cap without
/// reading the rest.
fn read_capped(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let cap = pdfcer_render::MAX_SOFT_PROOF_PROFILE_BYTES;
    let file = std::fs::File::open(path).map_err(|err| err.to_string())?;
    let mut bytes = Vec::new();
    file.take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    if bytes.len() > cap {
        return Err("the profile is larger than the 16 MiB a proof profile may be".to_owned());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::ViewerPreview;
    use pdfcer_render::RenderOptions;

    #[test]
    fn the_flag_never_turns_off_a_setting_that_preserves_cmyk() {
        let from_settings = RenderOptions::default().with_preserve_calibrated_cmyk(true);
        let unflagged = ViewerPreview::default().apply(from_settings);
        assert!(unflagged.policy().preserve_calibrated_cmyk);
        let flagged = ViewerPreview::default()
            .with_preserve_cmyk(true)
            .apply(RenderOptions::default());
        assert!(flagged.policy().preserve_calibrated_cmyk);
        let neither = ViewerPreview::default().apply(RenderOptions::default());
        assert!(!neither.policy().preserve_calibrated_cmyk);
    }
}

//! `set-text-annot-style` — restyle a sticky note, stamp or text box and
//! re-bake its appearance.

use super::*;
use pdfcer_core::edit::{TextAnnotStyle, TextAnnotStyleChange};

/// The style flags of `set-text-annot-style`.
#[derive(Debug, Clone, Default, clap::Args)]
pub(crate) struct TextAnnotStyleFlags {
    /// Sticky-note icon. `/Text` only.
    #[arg(long, value_name = "NAME")]
    pub(crate) icon: Option<StickyIconArg>,
    /// Colour as `RRGGBB` hex: a note's icon, a stamp's face, a text box's
    /// border.
    ///
    /// There is no `none`: the authoring model gives each of them a REQUIRED
    /// colour. To remove a text box's border use `--border-width 0`.
    #[arg(long, value_name = "RRGGBB")]
    pub(crate) color: Option<String>,
    /// **Label size in points** for a `/Stamp` or `/FreeText`.
    ///
    /// Refused by name on a `/Text` sticky note, which draws an icon and
    /// has no label to size. The new size is written to `/DA` and the
    /// appearance re-baked, so the size survives a later resize.
    #[arg(long, value_name = "POINTS")]
    pub(crate) font_size: Option<f64>,
    /// What a stamp does when the RESIZED label no longer fits its box:
    /// `grow` (default — widen the box), `shrink` (smaller text, same
    /// box), `clip` (cut the label).
    ///
    /// Only meaningful with `--font-size`, and refused without it. The
    /// author's original fit intent is NOT recorded anywhere in a PDF,
    /// so this is your choice rather than a recovered one.
    #[arg(long, value_enum)]
    pub(crate) stamp_fit: Option<StampFitArg>,
    /// Redraw a text box pdfcer cannot redraw faithfully, instead of
    /// refusing.
    ///
    /// Applies to a `/FreeText` another program drew (its wrapping cannot
    /// be recovered, so it is redrawn wrapped within its box) or one
    /// holding rich text (`/RC` and `/DS` are removed and the plain
    /// text is drawn). Without this flag such a box is left unchanged
    /// and the command exits 9.
    #[arg(long)]
    pub(crate) redraw_as_plain: bool,
    /// Opacity of the whole annotation, `0.0`–`1.0` (`/CA`); `none` removes
    /// it (opaque). Any of the three subtypes.
    #[arg(long, value_name = "0.0-1.0|none")]
    pub(crate) opacity: Option<String>,
    /// A text box's background as `RRGGBB` hex; `none` makes it
    /// transparent. `/FreeText` only, like the flags below.
    #[arg(long, value_name = "RRGGBB|none")]
    pub(crate) fill: Option<String>,
    /// A text box's border width in points; `0` removes the border. A box
    /// with no border gets one, in `--color` or else black.
    #[arg(long, value_name = "PT")]
    pub(crate) border_width: Option<f64>,
    /// A text box's border dash as `ON,OFF,...` lengths in points, or
    /// `solid`.
    #[arg(long, value_name = "ON,OFF,...|solid")]
    pub(crate) dash: Option<String>,
    /// A text box's text colour as `RRGGBB` hex.
    #[arg(long, value_name = "RRGGBB")]
    pub(crate) text_color: Option<String>,
    /// A text box's font: a Latin standard-14 face such as `Helvetica`,
    /// `Times-Bold` or `Courier-Oblique`.
    #[arg(long, value_name = "FACE")]
    pub(crate) font: Option<String>,
}

impl TextAnnotStyleFlags {
    fn is_empty(&self) -> bool {
        self.icon.is_none()
            && self.color.is_none()
            && self.font_size.is_none()
            && self.opacity.is_none()
            && self.fill.is_none()
            && self.border_width.is_none()
            && self.dash.is_none()
            && self.text_color.is_none()
            && self.font.is_none()
    }

    /// The core style these flags describe, or the message naming the bad
    /// flag.
    fn to_style(&self) -> Result<TextAnnotStyle, String> {
        if self.is_empty() {
            return Err("nothing to change: pass at least one style flag".to_owned());
        }
        if self.stamp_fit.is_some() && self.font_size.is_none() {
            return Err(
                "--stamp-fit only applies with --font-size: it decides what happens to \
                        the box when the RESIZED label no longer fits it"
                    .to_owned(),
            );
        }
        if self.font_size.is_some_and(|s| !s.is_finite() || s <= 0.0) {
            return Err("--font-size must be a positive number of points".to_owned());
        }
        if self.border_width.is_some_and(|w| !w.is_finite() || w < 0.0) {
            return Err("--border-width must be zero or a positive number of points".to_owned());
        }
        let color = |flag: &str, v: &Option<String>| {
            v.as_deref()
                .map(parse_color)
                .transpose()
                .map_err(|e| format!("--{flag}: {e}"))
        };
        Ok(TextAnnotStyle {
            icon: self.icon.map(StickyIconArg::to_core),
            color: color("color", &self.color)?,
            font_size: self.font_size,
            stamp_fit: self.stamp_fit.map(StampFitArg::to_fit),
            redraw_as_plain: self.redraw_as_plain,
            opacity: parse_opacity_edit(self.opacity.as_deref())?,
            fill: parse_color_edit("fill", self.fill.as_deref())?,
            border_width: self.border_width,
            dash: parse_dash_edit(self.dash.as_deref())?,
            text_color: color("text-color", &self.text_color)?.map(Into::into),
            font: self
                .font
                .as_deref()
                .map(resolve_latin_std14)
                .transpose()
                .map_err(|e| format!("--font: {e}"))?,
        })
    }
}

/// Implement `pdfcer set-text-annot-style`.
pub(crate) fn cmd_set_text_annot_style(
    input: &Path,
    page: usize,
    index: usize,
    flags: &TextAnnotStyleFlags,
    output: &Path,
    mode: SaveMode,
) -> u8 {
    let style = match flags.to_style() {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("pdfcer: {msg}");
            return exit::EDIT_REFUSED;
        }
    };
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let annot_id = match resolve_annotation(&session, input, page, index) {
        Ok(id) => id,
        Err(code) => return code,
    };
    let change = match session.set_text_annot_style(annot_id, &style) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let saved = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    println!(
        "set-text-annot-style {} page {page} index {index} -> {}",
        input.display(),
        output.display()
    );
    report_text_annot_style(input, &change);
    finish_edit(input, &saved)
}

/// What the re-bake decided (rule 11), then the machine-readable line.
fn report_text_annot_style(input: &Path, change: &TextAnnotStyleChange) {
    if let Some(fit) = change.stamp_label_fit
        && fit.is_inference()
    {
        report_stamp_label_fit(input, fit);
    }
    if change.appearance_was_foreign {
        eprintln!(
            "pdfcer: {}: the previous appearance was NOT one pdfcer would have drawn, and \
             re-baking has replaced it with pdfcer's plain rendering (a stamp's artwork, or a \
             text box redrawn under --redraw-as-plain, wrapped within its box).",
            input.display()
        );
    }
    if !change.rich_text_dropped.is_empty() {
        eprintln!(
            "pdfcer: {}: removed the text box's rich text ({}); it now shows its plain text.",
            input.display(),
            change.rich_text_dropped.join(", ")
        );
    }
    println!(
        "  obj={} subtype={} icon_written={} color_written={} font_size_written={} \
         opacity_written={} frame_written={} text_style_written={} \
         rect={:.2},{:.2},{:.2},{:.2} was_foreign={} rich_text_dropped={} appearance={}",
        change.annot_id.num,
        change.subtype,
        u32::from(change.icon_written),
        u32::from(change.color_written),
        u32::from(change.font_size_written),
        u32::from(change.opacity_written),
        u32::from(change.frame_written),
        u32::from(change.text_style_written),
        change.rect_after.llx,
        change.rect_after.lly,
        change.rect_after.urx,
        change.rect_after.ury,
        u32::from(change.appearance_was_foreign),
        change.rich_text_dropped.len(),
        match change.appearance {
            pdfcer_core::edit::AppearanceWrite::InPlace(_) => "in-place",
            pdfcer_core::edit::AppearanceWrite::Created(_) => "created",
            pdfcer_core::edit::AppearanceWrite::CopiedOnWrite { .. } => "copied",
            // `AppearanceWrite` is #[non_exhaustive].
            _ => "other",
        },
    );
}

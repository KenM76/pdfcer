//! The centre-line and centre-mark stroke flags shared by `group-style` and
//! `dimension-style`, and their `dimension-list` reporting.

use super::*;

use pdfcer_core::dimension::{CentreStyle, DimensionStyle, PartStroke};

/// The six centre-stroke flags, flattened into both style commands.
#[derive(clap::Args, Debug, Clone, Default)]
pub(crate) struct CentreStyleFlags {
    /// Dash pattern for centre lines, as `--dash`. Unset, a centre line
    /// draws the standard's long-short-long centre pattern, stretched to
    /// start and end on a long dash. `--dash` never changes centre lines.
    #[arg(long)]
    pub(crate) centre_line_dash: Option<String>,
    /// Stroke width of centre lines, in points. Unset, it follows
    /// `--line-width`.
    #[arg(long)]
    pub(crate) centre_line_width: Option<f64>,
    /// Colour of centre lines, as `--color`. Unset, it follows the lines'
    /// colour.
    #[arg(long)]
    pub(crate) centre_line_color: Option<String>,
    /// Dash pattern for centre marks (the cross and its extension lines),
    /// as `--dash`. Unset, a centre mark is solid.
    #[arg(long)]
    pub(crate) centre_mark_dash: Option<String>,
    /// Stroke width of centre marks, in points. Unset, it follows
    /// `--line-width`.
    #[arg(long)]
    pub(crate) centre_mark_width: Option<f64>,
    /// Colour of centre marks, as `--color`. Unset, it follows the lines'
    /// colour.
    #[arg(long)]
    pub(crate) centre_mark_color: Option<String>,
}

impl CentreStyleFlags {
    /// The fields given, validated; unset fields inherit.
    ///
    /// # Errors
    ///
    /// The operator-facing message naming the flag that failed.
    pub(crate) fn parse(&self) -> Result<CentreStyle, String> {
        let part = |dash: &Option<String>, width: Option<f64>, color: &Option<String>, name| {
            Ok::<_, String>(
                PartStroke::FOLLOW
                    .with_dash(dash.as_deref().map(parse_dash).transpose()?)
                    .with_width(width.map(|v| checked_metric(name, v)).transpose()?)
                    .with_color(color.as_deref().map(parse_device_color).transpose()?),
            )
        };
        Ok(CentreStyle::INHERIT
            .with_line(part(
                &self.centre_line_dash,
                self.centre_line_width,
                &self.centre_line_color,
                "centre-line-width",
            )?)
            .with_mark(part(
                &self.centre_mark_dash,
                self.centre_mark_width,
                &self.centre_mark_color,
                "centre-mark-width",
            )?))
    }
}

/// Clear the centre-style field `prop` names; other properties are no-ops.
pub(crate) fn clear_centre(prop: StylePropArg, c: &mut CentreStyle) {
    match prop {
        StylePropArg::CentreLineDash => c.line.dash = None,
        StylePropArg::CentreLineWidth => c.line.width = None,
        StylePropArg::CentreLineColor => c.line.color = None,
        StylePropArg::CentreMarkDash => c.mark.dash = None,
        StylePropArg::CentreMarkWidth => c.mark.width = None,
        StylePropArg::CentreMarkColor => c.mark.color = None,
        _ => {}
    }
}

/// The resolved centre strokes as `dimension-list --style` prints them, in
/// `StyleProvenance::each` order.
pub(crate) fn centre_style_values(s: &DimensionStyle) -> [(&'static str, String); 6] {
    let line = s.centre_line_stroke();
    let mark = s.centre_mark_stroke();
    [
        ("centre-line-dash", format_dash(line.dash)),
        ("centre-line-width", fmt_num(line.width)),
        ("centre-line-color", fmt_color(line.color)),
        ("centre-mark-dash", format_dash(mark.dash)),
        ("centre-mark-width", fmt_num(mark.width)),
        ("centre-mark-color", fmt_color(mark.color)),
    ]
}

/// ` dash_fit=TOKEN` for a centre line, how its pattern was laid along it;
/// empty for any other kind or an unknown group.
pub(crate) fn dash_fit_suffix(
    model: &pdfcer_core::dimension::DimensionModel,
    d: &pdfcer_core::dimension::DimensionRecord,
) -> String {
    let Some(group) = model.group(d.group) else {
        return String::new();
    };
    let style = pdfcer_core::dimension::resolve_style(group, &d.style);
    d.kind
        .centre_line_dash_fit(style)
        .map_or_else(String::new, |f| format!(" dash_fit={}", f.token()))
}

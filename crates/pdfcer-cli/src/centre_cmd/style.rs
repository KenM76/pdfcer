//! The centre-line and centre-mark stroke flags shared by `group-style` and
//! `dimension-style`, and their `dimension-list` reporting.

use super::*;

use pdfcer_core::dimension::{CentreMarkSize, CentreStyle, DimensionStyle, PartStroke};

/// The centre-stroke and centre-mark reach flags, flattened into both
/// style commands.
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
    /// Space between a centre mark's cross arm and the start of its
    /// extension line: `text:<multiple of the text height>` or
    /// `fixed:<points>`, as `--centre-mark-size`. Unset, the arm length.
    #[arg(long)]
    pub(crate) centre_mark_gap: Option<String>,
    /// How far a centre mark's extension lines run past the circle, as
    /// `--centre-mark-gap`. Unset, the arm length. When the gap leaves no
    /// room before that point the extension lines are not drawn, and
    /// `dimension-list` says `extension_lines_drawn=false`.
    #[arg(long)]
    pub(crate) centre_mark_extension: Option<String>,
    /// `true` draws a centre mark's extension lines in the centre-line dash
    /// (`--centre-line-dash`, else the standard's centre pattern), fitted to
    /// each line; the cross stays in the mark's dash. Unset, `false`.
    #[arg(long)]
    pub(crate) centre_mark_line_font: Option<bool>,
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
            )?)
            .with_mark_gap(reach("centre-mark-gap", self.centre_mark_gap.as_deref())?)
            .with_mark_extension(reach(
                "centre-mark-extension",
                self.centre_mark_extension.as_deref(),
            )?)
            .with_mark_line_font(self.centre_mark_line_font))
    }
}

/// Parse a `--centre-mark-gap` / `--centre-mark-extension` value.
fn reach(flag: &str, spec: Option<&str>) -> Result<Option<CentreMarkSize>, String> {
    spec.map(|s| {
        CentreMarkSize::parse(s).ok_or_else(|| {
            format!(
                "--{flag} `{s}` is not a size: use text:<multiple of the text height> (e.g. text:0.5) or fixed:<points> (e.g. fixed:4), positive"
            )
        })
    })
    .transpose()
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
        StylePropArg::CentreMarkGap => c.mark_gap = None,
        StylePropArg::CentreMarkExtension => c.mark_extension = None,
        StylePropArg::CentreMarkLineFont => c.mark_line_font = None,
        _ => {}
    }
}

/// The resolved centre strokes as `dimension-list --style` prints them, in
/// `StyleProvenance::each` order.
pub(crate) fn centre_style_values(s: &DimensionStyle) -> [(&'static str, String); 9] {
    let line = s.centre_line_stroke();
    let mark = s.centre_mark_stroke();
    [
        ("centre-line-dash", format_dash(line.dash)),
        ("centre-line-width", fmt_num(line.width)),
        ("centre-line-color", fmt_color(line.color)),
        ("centre-mark-dash", format_dash(mark.dash)),
        ("centre-mark-width", fmt_num(mark.width)),
        ("centre-mark-color", fmt_color(mark.color)),
        ("centre-mark-gap", fmt_num(s.centre_mark_gap())),
        ("centre-mark-extension", fmt_num(s.centre_mark_extension())),
        (
            "centre-mark-line-font",
            s.centre_mark_line_font().to_string(),
        ),
    ]
}

/// ` extension_lines_drawn=BOOL` for a ce dimension whose centre mark asks
/// for extension lines; empty otherwise or for an unknown group.
pub(crate) fn extension_lines_suffix(
    model: &pdfcer_core::dimension::DimensionModel,
    d: &pdfcer_core::dimension::DimensionRecord,
) -> String {
    let Some(group) = model.group(d.group) else {
        return String::new();
    };
    let style = pdfcer_core::dimension::resolve_style(group, &d.style);
    d.kind
        .centre_mark_extension_lines_drawn(style)
        .map_or_else(String::new, |on| format!(" extension_lines_drawn={on}"))
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

//! A list-box widget appearance (ISO 32000-1 §12.7.4.4): the box chrome,
//! then [`vartext::build_list_box_text`]'s rows inside the border.

use super::{
    FieldAppearance, FontResource, Quadding, Rect, VarTextError, WidgetChrome, field_box_art,
    field_has_border, text_form_dict, vartext,
};
use crate::edit::BorderStyle;
use crate::field_display::ListRows;

/// Build a `w × h` list box showing `rows`, with `/DA` `da` and `/Q` `quad`.
///
/// The rows are clipped to the box less its frame: one border width, two for
/// a `Beveled` or `Inset` border, whose shading band sits inside the frame.
///
/// # Errors
///
/// As [`vartext::build_list_box_text`].
pub(crate) fn build_list_box_appearance(
    w: f64,
    h: f64,
    rows: &ListRows,
    da: &[u8],
    quad: Quadding,
    fonts: &[FontResource],
    chrome: WidgetChrome,
) -> Result<FieldAppearance, VarTextError> {
    let (w, h) = (w.max(1.0), h.max(1.0));
    let inset = if field_has_border(&chrome) {
        let bw = chrome.border_width().unwrap_or(0.0);
        match chrome.border.unwrap_or_default().style {
            BorderStyle::Beveled | BorderStyle::Inset => 2.0 * bw,
            _ => bw,
        }
    } else {
        0.0
    }
    .min(w.min(h) / 2.0 - 0.5)
    .max(0.0);
    let area = Rect {
        llx: inset,
        lly: inset,
        urx: w - inset,
        ury: h - inset,
    };
    let lb = vartext::build_list_box_text(
        area,
        &rows.options,
        &rows.selected,
        rows.top_index,
        da,
        quad,
        fonts,
    )?;
    let mut content = field_box_art(&chrome, w, h, None);
    content.extend_from_slice(&lb.text.content);
    let bbox = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: w,
        ury: h,
    };
    Ok(FieldAppearance {
        ap_dict: text_form_dict(bbox, lb.text.resources),
        content,
        applied_autosize: lb.text.applied_autosize,
        applied_autosize_bound: None,
        da_colour_unmodelled: lb.text.da_colour_unmodelled,
        unencodable_chars: lb.text.unencodable_chars,
    })
}

//! List-box appearance layout (ISO 32000-1 §12.7.4.4): one row per option,
//! scrolled to a first row, with selected rows on a highlight band.

use pdfcer_model::page_tree::Rect;
use pdfcer_model::writer::content::{ContentBuilder, Paint};

use super::{
    DefaultAppearance, Face, FontResource, LINE_FACTOR, Quadding, TEXT_PAD, VarTextAppearance,
    VarTextError, align_x, auto_size, da_face, font_resources, measure, parse_default_appearance,
    visible_line_count,
};

/// The DeviceRGB fill behind a selected list-box row.
///
/// A producer convention, not a spec value: §12.7.4.4 says selected options
/// are shown "highlighted" and fixes no colour. This is the light blue the
/// common interactive-form producer bakes into its own list-box streams, so a
/// pdfcer-drawn list box reads the same as one it drew.
pub const LIST_BOX_HIGHLIGHT_RGB: (f64, f64, f64) = (0.600_006, 0.756_866, 0.854_904);

/// A laid-out list box: the appearance body and which rows it shows.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ListBoxAppearance {
    /// The `/Tx BMC … EMC` body, its font resources and its disclosures.
    pub text: VarTextAppearance,
    /// The option index drawn in the top row.
    pub first_row: usize,
    /// How many rows were drawn (never more than fit in `area`).
    pub rows_drawn: usize,
}

/// The option index a list box shows first.
///
/// `top_index` (`/TI`) wins when it names an option. Otherwise the list
/// starts at 0 unless the first selected option would be below the fold, in
/// which case the last window that contains it is shown. `visible` is at
/// least 1, so the result is always `< count` when `count > 0`.
#[must_use]
pub fn list_box_first_row(
    count: usize,
    visible: usize,
    top_index: Option<usize>,
    first_selected: Option<usize>,
) -> usize {
    if let Some(t) = top_index.filter(|&t| t < count) {
        return t;
    }
    let window = visible.clamp(1, count.max(1));
    match first_selected.filter(|&s| s < count) {
        Some(s) if s >= window => s.min(count - window),
        _ => 0,
    }
}

/// Lay out a list box's `options` inside `area` (the widget box less its
/// border), §12.7.4.4.
///
/// Rows are `size × 1.15` tall where `size` is the `/DA` size, or
/// [`auto_size`] of the area height when the `/DA` asks for auto-size (the
/// chosen size is reported in `applied_autosize`). The first row is
/// [`list_box_first_row`]; only rows that fit are encoded and drawn, all
/// clipped to `area`. Each row in `selected` that is drawn sits on a
/// [`LIST_BOX_HIGHLIGHT_RGB`] band spanning the area's width. Indices in
/// `selected` and a `top_index` past the end are ignored; the caller reports
/// them.
///
/// # Errors
///
/// As [`super::build_variable_text`].
pub fn build_list_box_text(
    area: Rect,
    options: &[String],
    selected: &[usize],
    top_index: Option<usize>,
    da: &[u8],
    quad: Quadding,
    resources: &[FontResource],
) -> Result<ListBoxAppearance, VarTextError> {
    let parsed = parse_default_appearance(da)?;
    let face = da_face(&parsed, resources)?;
    let h = area.height();
    let (size, applied_autosize) = if parsed.font_size == 0.0 {
        (auto_size(h), Some(auto_size(h)))
    } else {
        (parsed.font_size, None)
    };
    let lh = size * LINE_FACTOR;
    let visible = visible_line_count(h, size);
    let first_selected = selected
        .iter()
        .copied()
        .filter(|&s| s < options.len())
        .min();
    let first = list_box_first_row(options.len(), visible, top_index, first_selected);
    let rows: Vec<usize> = (first..options.len()).take(visible).collect();
    let geom = RowGeometry {
        area,
        lh,
        glyph_drop: (lh - face.ascent_at(size) - face.descent_at(size)) / 2.0
            + face.ascent_at(size),
    };
    let (content, unencodable_chars) =
        list_box_content(&geom, options, &rows, selected, &parsed, face, size, quad);
    Ok(ListBoxAppearance {
        text: VarTextAppearance {
            content,
            resources: font_resources(&parsed.font_name, face.font),
            applied_autosize,
            applied_autosize_bound: None,
            da_colour_unmodelled: parsed.color_unmodelled,
            unencodable_chars,
            used_font: face.font,
            used_size: size,
        },
        first_row: first,
        rows_drawn: rows.len(),
    })
}

/// Where list rows sit: `lh`-tall bands from the top of `area`, glyphs
/// centred in each (`glyph_drop` = band top to baseline).
struct RowGeometry {
    area: Rect,
    lh: f64,
    glyph_drop: f64,
}

impl RowGeometry {
    fn row_top(&self, i: usize) -> f64 {
        self.area.ury - TEXT_PAD - i as f64 * self.lh
    }
}

/// The clipped content: highlight bands, then one text row per `rows`
/// entry. Returns the bytes and the unencodable-character count.
#[allow(clippy::too_many_arguments)] // all layout inputs, resolved once by the caller.
fn list_box_content(
    g: &RowGeometry,
    options: &[String],
    rows: &[usize],
    selected: &[usize],
    parsed: &DefaultAppearance,
    face: Face<'_>,
    size: f64,
    quad: Quadding,
) -> (Vec<u8>, usize) {
    let (area, lh) = (g.area, g.lh);
    let w = area.width();
    let mut b = ContentBuilder::new();
    b.begin_marked_content(b"Tx");
    b.save_state();
    b.rect(area.llx, area.lly, w, area.height());
    b.clip_nonzero();
    b.paint(Paint::NoPaint);
    let lit: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| selected.contains(r))
        .map(|(i, _)| i)
        .collect();
    if !lit.is_empty() {
        let (r, gr, bl) = LIST_BOX_HIGHLIGHT_RGB;
        b.set_fill_rgb(r, gr, bl);
        for i in lit {
            b.rect(area.llx, g.row_top(i) - lh, w, lh);
        }
        b.paint(Paint::Fill);
    }
    b.begin_text();
    b.set_font(&parsed.font_name, size);
    match parsed.color {
        Some(c) => c.apply_fill(&mut b),
        None => b.set_fill_gray(0.0),
    }
    let mut unencodable_chars = 0usize;
    let mut pen = (0.0, 0.0);
    for (i, &opt) in rows.iter().enumerate() {
        let flat: String = options
            .get(opt)
            .map_or("", String::as_str)
            .chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect();
        let (codes, miss) = face.encode(&flat);
        unencodable_chars += miss;
        let x = area.llx + align_x(quad, w, measure(face, size, &codes));
        let y = g.row_top(i) - g.glyph_drop;
        if i == 0 {
            b.set_text_matrix(1.0, 0.0, 0.0, 1.0, x, y);
        } else {
            b.text_move(x - pen.0, y - pen.1);
        }
        pen = (x, y);
        b.show_text(&face.show_bytes(&codes));
    }
    b.end_text();
    b.restore_state();
    b.end_marked_content();
    (b.into_bytes(), unencodable_chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_row_prefers_a_valid_top_index_then_scrolls_the_selection_into_view() {
        assert_eq!(list_box_first_row(6, 3, Some(1), Some(4)), 1);
        assert_eq!(list_box_first_row(6, 3, Some(99), Some(4)), 3);
        assert_eq!(list_box_first_row(6, 3, None, Some(2)), 0);
        assert_eq!(list_box_first_row(6, 3, None, Some(5)), 3);
        assert_eq!(list_box_first_row(6, 3, None, None), 0);
        assert_eq!(list_box_first_row(2, 5, None, Some(1)), 0);
        assert_eq!(list_box_first_row(0, 1, Some(0), Some(0)), 0);
    }
}

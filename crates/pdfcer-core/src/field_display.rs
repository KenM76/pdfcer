//! What a widget appearance shows for a field's stored value: the output of
//! its `/AA /F` format helper (ISO 32000-1 §12.6.3 Table 196), and a list
//! box's row window and selection (§12.7.4.4 Table 231). `/V` is never
//! changed; what pdfcer declined to infer is reported in [`DisplayNotes`].

use crate::edit::{decode_text_string, match_option};
use crate::form_script::calc::CommaPolicy;
use crate::form_script::format::{self, FormatOutcome};
use crate::form_script::{ScriptClass, inventory};
use crate::forms::{Field, FieldFlags, FieldType, FieldValue};
use crate::graph::ObjectGraph;
use crate::object::Object;
use crate::view::DocumentView;

/// What one widget appearance drew instead of, or ignored from, the stored
/// field state. Empty ([`Self::is_empty`]) for a plain draw.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct DisplayNotes {
    /// A recognised `AF*_Format` helper's output was drawn in place of `/V`.
    pub formatted: bool,
    /// The field has a format action pdfcer did not reproduce, so `/V` was
    /// drawn unchanged; the reason, in plain words.
    pub unformatted: Option<String>,
    /// A list box's `/TI` named no option (negative or past the end of
    /// `/Opt`) and was ignored.
    pub ignored_top_index: Option<i64>,
    /// A list box's `/I` entries that named no option and were ignored, in
    /// file order.
    pub ignored_selected: Vec<i64>,
    /// The appearance was drawn from the field's rich value `/RV`
    /// (§12.7.3.4).
    pub rich_text: bool,
    /// Rich-text markup that was not drawn, once each: `<name>` for an
    /// element outside Table 223, a style property by name, or
    /// `name: value` for a value outside Table 225.
    pub rich_ignored: Vec<String>,
    /// Rich-text fonts drawn with another font, as `requested -> used`.
    pub rich_fonts: Vec<String>,
    /// The `/RV` would not parse or its stream would not decode, so `/V` was
    /// drawn as plain text; why.
    pub rich_unreadable: Option<String>,
    /// A signed signature field drawn with no `/AP`: the lines taken from
    /// its signature dictionary's `/Name` and `/M` (§12.8.1 Table 252), in
    /// drawing order. pdfcer composed them; they claim nothing about
    /// validity.
    pub signature_text: Vec<String>,
    /// What a signed signature field's drawing left out, and why: a missing
    /// `/Name` or `/M`, a document timestamp's ignored properties, text that
    /// did not fit.
    pub signature_omitted: Vec<String>,
}

impl DisplayNotes {
    /// Whether nothing was formatted, declined or ignored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// How many list-box indices (`/TI` and `/I`) were ignored.
    #[must_use]
    pub fn ignored_list_indices(&self) -> usize {
        self.ignored_selected.len() + usize::from(self.ignored_top_index.is_some())
    }
}

/// Per-field [`DisplayNotes`] summed over one command's redraws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct DisplayCounts {
    /// Fields drawn with a recognised format helper's output.
    pub formatted: usize,
    /// Fields whose format action pdfcer did not reproduce, so `/V` was drawn
    /// unchanged.
    pub unformatted: usize,
    /// List-box `/TI` and `/I` entries that named no option and were ignored.
    pub list_indices_ignored: usize,
    /// Fields drawn from their rich value `/RV`.
    pub rich_text: usize,
    /// Rich-text markup items not drawn ([`DisplayNotes::rich_ignored`]).
    pub rich_markup_ignored: usize,
    /// Rich-text fonts drawn with another font
    /// ([`DisplayNotes::rich_fonts`]).
    pub rich_fonts_substituted: usize,
    /// Rich-text fields whose `/RV` would not parse, drawn as plain text.
    pub rich_unreadable: usize,
}

impl DisplayCounts {
    /// Fold one field's notes in.
    pub(crate) fn add(&mut self, notes: &DisplayNotes) {
        self.formatted += usize::from(notes.formatted);
        self.unformatted += usize::from(notes.unformatted.is_some());
        self.list_indices_ignored += notes.ignored_list_indices();
        self.rich_text += usize::from(notes.rich_text);
        self.rich_markup_ignored += notes.rich_ignored.len();
        self.rich_fonts_substituted += notes.rich_fonts.len();
        self.rich_unreadable += usize::from(notes.rich_unreadable.is_some());
    }
}

/// A list box's selection and `/TI` as the calling command is about to
/// write them, for a redraw whose `Field` snapshot predates the write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListSelection {
    /// Selected `/Opt` indices.
    pub indices: Vec<i64>,
    /// The `/TI` being written; `None` = absent.
    pub top_index: Option<i64>,
}

/// The rows a list box draws, resolved against its `/Opt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListRows {
    pub options: Vec<String>,
    pub selected: Vec<usize>,
    pub top_index: Option<usize>,
}

/// A choice field without the Combo flag (bit 18).
pub(crate) fn is_list_box(field: &Field) -> bool {
    field.field_type == Some(FieldType::Choice) && !field.flags.has(FieldFlags::COMBO)
}

/// Whether a format action governs what `field` displays: a text field, or
/// a combo box with Edit (bit 19) — the two kinds whose value is free text.
fn takes_format(field: &Field) -> bool {
    match field.field_type {
        Some(FieldType::Text) => true,
        Some(FieldType::Choice) => {
            field.flags.has(FieldFlags::COMBO) && field.flags.has(FieldFlags::EDIT)
        }
        _ => false,
    }
}

/// The text to draw for `stored` and whether to draw it red.
///
/// A recognised helper's output replaces `stored`; any other format action
/// leaves `stored` and records why in `notes`. An empty value is drawn empty
/// and is not reported.
pub(crate) fn shown_value(
    view: &DocumentView<'_>,
    field: &Field,
    stored: &str,
    notes: &mut DisplayNotes,
) -> (String, bool) {
    if stored.is_empty() || !takes_format(field) {
        return (stored.to_owned(), false);
    }
    match inventory::format_class(view, field) {
        None => (stored.to_owned(), false),
        Some(ScriptClass::Format(helper)) => {
            match format::render(&helper, stored, CommaPolicy::default()) {
                FormatOutcome::Rendered(f) => {
                    notes.formatted = true;
                    (f.text, f.red)
                }
                declined => {
                    notes.unformatted = declined.decline_reason();
                    (stored.to_owned(), false)
                }
            }
        }
        Some(_) => {
            notes.unformatted = Some(
                "the format action is not a recognised AF*_Format call, so the stored value \
                 is drawn unformatted"
                    .to_owned(),
            );
            (stored.to_owned(), false)
        }
    }
}

/// `da` with a red non-stroking colour appended; the last colour operator in
/// a `/DA` is the one the text is drawn in.
pub(crate) fn red_da(da: &[u8]) -> Vec<u8> {
    let mut out = da.to_vec();
    out.extend_from_slice(b" 1 0 0 rg");
    out
}

/// The `/TI` written on `field`'s own dictionary, `None` when absent.
pub(crate) fn raw_top_index(view: &DocumentView<'_>, field: &Field) -> Option<i64> {
    view.resolved(field.id)
        .as_dict()?
        .get(b"TI")
        .map(|o| view.resolve(o))
        .and_then(Object::as_int)
}

/// A list box's rows. The selection is the options `/V` names (export value
/// first, then display); when `/V` names none, the in-range `/I` entries.
/// Table 231: where `/I` and `/V` differ, `/V` is used. Out-of-range `/I`
/// and `/TI` values are recorded in `notes` and ignored.
pub(crate) fn list_rows(
    field: &Field,
    over: Option<&ListSelection>,
    raw_ti: Option<i64>,
    notes: &mut DisplayNotes,
) -> ListRows {
    let options: Vec<String> = field
        .options
        .iter()
        .map(|o| decode_text_string(&o.display).text)
        .collect();
    let n = options.len();
    let in_range = |i: i64| usize::try_from(i).ok().filter(|&u| u < n);
    let (indices, ti) = match over {
        Some(o) => (o.indices.clone(), o.top_index),
        None => (field.selected_indices.clone(), raw_ti),
    };
    notes.ignored_selected = indices
        .iter()
        .copied()
        .filter(|&i| in_range(i).is_none())
        .collect();
    let top_index = ti.and_then(in_range);
    notes.ignored_top_index = ti.filter(|_| top_index.is_none());
    let from_v = if over.is_some() {
        Vec::new()
    } else {
        v_selection(field)
    };
    let mut selected = if from_v.is_empty() {
        indices.into_iter().filter_map(in_range).collect()
    } else {
        from_v
    };
    selected.sort_unstable();
    selected.dedup();
    ListRows {
        options,
        selected,
        top_index,
    }
}

/// The `/Opt` indices `/V` names.
fn v_selection(field: &Field) -> Vec<usize> {
    let items: Vec<String> = match &field.value {
        FieldValue::Choice(items) => items.iter().map(|b| decode_text_string(b).text).collect(),
        FieldValue::Text(b) => vec![decode_text_string(b).text],
        _ => Vec::new(),
    };
    items
        .iter()
        .filter_map(|v| match_option(&field.options, v).map(|(i, _)| i))
        .collect()
}

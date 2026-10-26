//! Appearances for form-field widgets that carry no `/AP`, built at view
//! time and never written to the file.
//!
//! ISO 32000-1 §12.7.3.3: for a variable-text field without an appearance
//! stream "the conforming reader shall construct an appearance stream
//! dynamically at viewing time"; §12.5.2's `/AP` row lets a handler supply
//! its own appearance for any widget. The obligation is on pixels, not
//! bytes: [`WidgetAppearances`] lays a widget out exactly as
//! [`crate::edit::EditSession::regenerate_appearances`] would, and hands the
//! result to a renderer. The document is untouched.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::annot_author::{self, CheckStyle, WidgetChrome};
use crate::edit::{EditSession, choice_display_text, decode_text_string};
use crate::forms::{self, ButtonKind, Field, FieldFlags, FieldType, FieldValue, Widget};
use crate::object::{Dict, Name, ObjId, Object};
use crate::vartext::FontResource;
use crate::view::DocumentView;

/// The `/DA` used when neither the field nor the form states one.
const FALLBACK_DA: &[u8] = b"/Helv 0 Tf 0 g";

/// Which kind of widget an appearance was synthesized for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SynthesizedKind {
    /// A text field (`/FT /Tx`).
    Text,
    /// A list or combo box (`/FT /Ch`).
    Choice,
    /// A check box.
    CheckBox,
    /// A radio button.
    RadioButton,
    /// A push button (its `/MK /CA` caption).
    PushButton,
}

/// A view-time appearance for one widget: a form XObject's dict and its
/// content, ready to place on the widget's `/Rect` per §12.5.5.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SynthesizedAppearance {
    /// The form dict: `/BBox`, `/Resources`, and `/Matrix` for a
    /// quarter-turned widget (`/MK /R`).
    pub ap_dict: Dict,
    /// The content-stream bytes (unfiltered).
    pub content: Vec<u8>,
    /// What was drawn.
    pub kind: SynthesizedKind,
}

/// A document's form, parsed once, from which per-widget appearances are
/// synthesized.
#[derive(Debug, Clone)]
pub struct WidgetAppearances {
    fields: Vec<Field>,
    /// Widget id -> (field index, widget index).
    index: HashMap<ObjId, (usize, usize)>,
    fonts: Vec<FontResource>,
    default_da: Vec<u8>,
}

impl WidgetAppearances {
    /// Parse `view`'s interactive form; `None` when it has none.
    #[must_use]
    pub fn read(view: &DocumentView<'_>) -> Option<Self> {
        let form = forms::parse_acroform(view)?;
        let mut index = HashMap::new();
        for (fi, field) in form.fields.iter().enumerate() {
            for (wi, widget) in field.widgets.iter().enumerate() {
                index.insert(widget.id, (fi, wi));
            }
        }
        Some(Self {
            fonts: EditSession::dr_fonts_of(view, view),
            default_da: form
                .default_appearance
                .clone()
                .unwrap_or_else(|| FALLBACK_DA.to_vec()),
            fields: form.fields,
            index,
        })
    }

    /// The appearance pdfcer would draw for widget `widget_id`, with the
    /// form's `/DR` fonts bound into its resources. `None` for an unknown
    /// widget, a signature field, or a widget with no `/Rect`.
    #[must_use]
    pub fn appearance(
        &self,
        view: &DocumentView<'_>,
        widget_id: ObjId,
    ) -> Option<SynthesizedAppearance> {
        let &(fi, wi) = self.index.get(&widget_id)?;
        let field = self.fields.get(fi)?;
        let widget = field.widgets.get(wi)?;
        let rect = widget.rect?;
        let quarter = widget.rotation.unwrap_or(0).rem_euclid(360);
        let (w, h) = if quarter == 90 || quarter == 270 {
            (rect.height(), rect.width())
        } else {
            (rect.width(), rect.height())
        };
        let mut out = self.draw(field, widget, w, h)?;
        EditSession::bind_dr_fonts_of(view, view, &mut out.ap_dict);
        if let Some(m) = EditSession::quarter_turn_matrix(quarter) {
            out.ap_dict.insert(
                Name::from(b"Matrix"),
                Object::Array(m.iter().map(|v| Object::Real(*v)).collect()),
            );
        }
        Some(out)
    }

    fn draw(
        &self,
        field: &Field,
        widget: &Widget,
        w: f64,
        h: f64,
    ) -> Option<SynthesizedAppearance> {
        let chrome = chrome_of(widget);
        let da = field
            .default_appearance
            .as_deref()
            .unwrap_or(&self.default_da);
        let (kind, ap) = match field.field_type? {
            FieldType::Text => {
                let value = match &field.value {
                    FieldValue::Text(b) => decode_text_string(b).text,
                    _ => String::new(),
                };
                let multiline = field.flags.has(FieldFlags::MULTILINE);
                (
                    SynthesizedKind::Text,
                    self.text(field, &value, da, multiline, w, h, chrome)?,
                )
            }
            FieldType::Choice => {
                let value = choice_display_text(field).unwrap_or_default();
                let multiline = !field.flags.has(FieldFlags::COMBO);
                (
                    SynthesizedKind::Choice,
                    self.text(field, &value, da, multiline, w, h, chrome)?,
                )
            }
            FieldType::Button => return self.button(field, widget, da, w, h, chrome),
            FieldType::Signature => return None,
        };
        Some(SynthesizedAppearance {
            ap_dict: ap.ap_dict,
            content: ap.content,
            kind,
        })
    }

    #[allow(clippy::too_many_arguments)] // the layout inputs, one each.
    fn text(
        &self,
        field: &Field,
        value: &str,
        da: &[u8],
        multiline: bool,
        w: f64,
        h: f64,
        chrome: WidgetChrome,
    ) -> Option<annot_author::FieldAppearance> {
        let shown = displayed_text(field, value);
        annot_author::build_field_text_appearance(
            w,
            h,
            &shown,
            da,
            field.quadding,
            multiline,
            comb_cells(field, multiline),
            &self.fonts,
            chrome,
        )
        .ok()
    }

    fn button(
        &self,
        field: &Field,
        widget: &Widget,
        da: &[u8],
        w: f64,
        h: f64,
        chrome: WidgetChrome,
    ) -> Option<SynthesizedAppearance> {
        let kind = field.button_kind.unwrap_or(ButtonKind::Check);
        if kind == ButtonKind::Push {
            let caption = widget
                .caption
                .as_deref()
                .map(|c| decode_text_string(c).text)
                .unwrap_or_default();
            let ap =
                annot_author::build_push_button_appearance(w, h, &caption, da, &self.fonts, chrome)
                    .ok()?;
            return Some(SynthesizedAppearance {
                ap_dict: ap.ap_dict,
                content: ap.content,
                kind: SynthesizedKind::PushButton,
            });
        }
        let caption_style = widget
            .caption
            .as_deref()
            .and_then(|c| c.first().copied())
            .and_then(CheckStyle::from_mk_caption_char);
        let (on, off, kind) = if kind == ButtonKind::Radio {
            let style = caption_style.unwrap_or(CheckStyle::Circle);
            let (off, on) = annot_author::build_radio_button_appearances(w, h, style, chrome);
            (on, off, SynthesizedKind::RadioButton)
        } else {
            let style = caption_style.unwrap_or(CheckStyle::Check);
            let (off, on) = annot_author::build_check_box_appearances(w, h, style, chrome);
            (on, off, SynthesizedKind::CheckBox)
        };
        let state = if button_is_on(field, widget, kind) {
            on
        } else {
            off
        };
        Some(SynthesizedAppearance {
            ap_dict: state.ap_dict,
            content: state.content,
            kind,
        })
    }
}

/// Whether a check box or radio button is drawn in its on state: its `/AS`
/// names a state other than `Off`; a check box with no `/AS` falls back to
/// the field's `/V` (§12.7.4.2.3).
fn button_is_on(field: &Field, widget: &Widget, kind: SynthesizedKind) -> bool {
    if let Some(state) = &widget.appearance_state {
        return state.as_slice() != b"Off";
    }
    kind == SynthesizedKind::CheckBox
        && matches!(&field.value, FieldValue::Name(v) if v.as_slice() != b"Off")
}

/// The border and colours `/MK` and `/BS` give a widget.
fn chrome_of(widget: &Widget) -> WidgetChrome {
    let chrome = WidgetChrome::new(widget.background, widget.border_color);
    let chrome = match widget.border {
        Some(border) => chrome.with_border(border),
        None => chrome,
    };
    match &widget.border_dash {
        Some(dash) => chrome.with_border_dash(dash.clone()),
        None => chrome,
    }
}

/// The text a field shows for `text`: a `Password` field's value is
/// "echoed in some unreadable form, such as asterisks" (§12.7.4.3 Table 228,
/// bit 14).
pub(crate) fn displayed_text<'a>(field: &Field, text: &'a str) -> Cow<'a, str> {
    if field.flags.has(FieldFlags::PASSWORD) {
        Cow::Owned("*".repeat(text.chars().filter(|c| !matches!(c, '\r' | '\n')).count()))
    } else {
        Cow::Borrowed(text)
    }
}

/// The comb cell count, only where Table 228 bit 25 permits a comb: `/MaxLen`
/// present and positive, Multiline/Password/FileSelect clear. An ill-formed
/// comb is drawn as plain text.
pub(crate) fn comb_cells(field: &Field, multiline: bool) -> Option<usize> {
    let f = field.flags;
    (f.has(FieldFlags::COMB)
        && !multiline
        && !f.has(FieldFlags::MULTILINE)
        && !f.has(FieldFlags::PASSWORD)
        && !f.has(FieldFlags::FILE_SELECT))
    .then_some(field.max_len)
    .flatten()
    .and_then(|n| usize::try_from(n).ok())
    .filter(|&n| n > 0)
}

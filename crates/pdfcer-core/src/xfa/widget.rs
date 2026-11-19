//! What kind of entry an XFA field takes, and the values that entry writes
//! (XFA 3.3 ui p.877, widgets pp.478-491; checkButton pp.650-651; items
//! pp.758-759; choiceList pp.652-653; exclGroup p.715; spec RAG
//! `adobe_ext__xfa_appearance_drawing.md` §8, `adobe_ext__xfa_widgets_paint.md`).

use super::paint::{check_states, choice_items, widget};
use super::{Template, XfaNode};

/// The entry a field offers, read from its `ui` widget (p.877).
///
/// Every value it carries is what [`crate::edit::EditSession::fill_xfa_field`]
/// should be given to select that choice.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FieldWidget {
    /// A `textEdit`, or a field with no `ui` and text or no value (p.478).
    Text {
        /// `multiLine="1"` (a field's default is single-line, p.865).
        multiline: bool,
        /// The value `text` element's positive `maxChars` (p.863).
        max_chars: Option<usize>,
    },
    /// A `numericEdit`, or no `ui` over a `decimal`, `float` or `integer`
    /// value (defaultUi, pp.490-491).
    Numeric,
    /// A `dateTimeEdit`, or no `ui` over a `date`, `dateTime` or `time` value.
    Date,
    /// A `checkButton`, or no `ui` over a `boolean` value.
    CheckButton {
        /// The value that means on: the first `items` entry, else `1`.
        on: String,
        /// The value that means off: the second `items` entry (null when
        /// absent, p.759), else `0` when there is no `items`.
        off: String,
        /// The third `items` entry when it is not empty. Always `None` in an
        /// exclusion group, whose members ignore it (p.759).
        neutral: Option<String>,
        /// The enclosing `exclGroup`'s path when the field is a radio button.
        /// The group is what binds: select this member by filling the group's
        /// path with `on` (p.715).
        group: Option<Vec<usize>>,
    },
    /// A `choiceList`.
    ChoiceList {
        /// `(display, save)` pairs, positionally matched (p.484); one list
        /// serves as both when there is no `save="1"` list. Read after a
        /// merge so `bindItems` lists are filled in.
        items: Vec<(String, String)>,
        /// How the list opens (p.653).
        open: ChoiceOpen,
        /// `textEntry="1"`: a value outside the list may be typed (never for
        /// `always` or `multiSelect`, p.484).
        text_entry: bool,
    },
    /// Any other widget (`passwordEdit`, `imageEdit`, `signature`,
    /// `button`, `barcode`), by its element name.
    Other(String),
}

/// A `choiceList`'s `open` attribute (p.653).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ChoiceOpen {
    /// `userControl` (the default): a drop-down opened by its button.
    #[default]
    UserControl,
    /// `onEntry`: a drop-down that opens when the field is entered.
    OnEntry,
    /// `always`: the list is always shown, one selection.
    Always,
    /// `multiSelect`: the list is always shown, several selections. Give
    /// [`crate::edit::EditSession::fill_xfa_field`] the chosen save values
    /// one per line, joined by `'\n'`.
    MultiSelect,
}

/// The selections in a `multiSelect` choice list's value: one per line, as a
/// bound data group's values are joined (XFA 3.3 p.198). A one-line value
/// that is not itself a save value is read space-separated, the form of the
/// widget chapter's example (p.485), so `"bananas pears"` is two selections
/// and `"New York"` stays one when it is in `save`.
pub(in crate::xfa) fn multi_select_values<'a>(value: &'a str, save: &[&str]) -> Vec<&'a str> {
    if value.contains('\n') || save.contains(&value) {
        value.lines().filter(|l| !l.is_empty()).collect()
    } else {
        value.split(' ').filter(|l| !l.is_empty()).collect()
    }
}

impl ChoiceOpen {
    /// Whether only the selection is shown until the list is opened.
    #[must_use]
    pub fn is_drop_down(self) -> bool {
        matches!(self, ChoiceOpen::UserControl | ChoiceOpen::OnEntry)
    }

    fn parse(s: Option<&str>) -> Self {
        match s.map(str::trim) {
            Some("onEntry") => ChoiceOpen::OnEntry,
            Some("always") => ChoiceOpen::Always,
            Some("multiSelect") => ChoiceOpen::MultiSelect,
            _ => ChoiceOpen::UserControl,
        }
    }
}

/// The entry the `field` at `path` takes; `None` when `path` does not lead
/// to a `field`.
///
/// Pass the merged template ([`crate::xfa::MergedForm::template`]) so
/// `bindItems` lists read as filled, and a [`crate::xfa::LayoutItem::path`]
/// or a [`crate::xfa::BoundValue::path`].
#[must_use]
pub fn field_widget(template: &Template, path: &[usize]) -> Option<FieldWidget> {
    let node = template.node_at(path).filter(|n| n.tag == "field")?;
    Some(match widget_tag(node) {
        "textEdit" => text(node),
        "numericEdit" => FieldWidget::Numeric,
        "dateTimeEdit" => FieldWidget::Date,
        "checkButton" => check(template, node, path),
        "choiceList" => choice(node),
        other => FieldWidget::Other(other.to_owned()),
    })
}

/// The widget element a field takes: its `ui` child's, else its value's
/// default.
pub(in crate::xfa) fn widget_tag(node: &XfaNode) -> &str {
    match widget(node) {
        Some(w) if w.tag != "defaultUi" => w.tag.as_str(),
        _ => default_ui(node),
    }
}

/// The widget a field with no `ui` widget (or a `defaultUi`) takes from its
/// value's content type (pp.478, 490-491).
fn default_ui(node: &XfaNode) -> &'static str {
    let content = node.child("value").and_then(|v| v.children.first());
    match content.map(|c| c.tag.as_str()) {
        Some("boolean") => "checkButton",
        Some("date" | "dateTime" | "time") => "dateTimeEdit",
        Some("decimal" | "float" | "integer") => "numericEdit",
        Some("image") => "imageEdit",
        _ => "textEdit",
    }
}

fn text(node: &XfaNode) -> FieldWidget {
    let multiline = widget(node)
        .and_then(|w| w.attr("multiLine"))
        .is_some_and(|m| m.trim() == "1");
    let max_chars = node
        .child("value")
        .and_then(|v| v.child("text"))
        .and_then(|t| t.attr("maxChars"))
        .and_then(|m| m.trim().parse::<usize>().ok())
        .filter(|&n| n > 0);
    FieldWidget::Text {
        multiline,
        max_chars,
    }
}

fn check(template: &Template, node: &XfaNode, path: &[usize]) -> FieldWidget {
    let group = path
        .split_last()
        .map(|(_, parent)| parent)
        .filter(|p| template.node_at(p).is_some_and(|n| n.tag == "exclGroup"))
        .map(<[usize]>::to_vec);
    let (on, off, neutral) = match check_states(node) {
        Some([on, off, neutral]) => (on, off, Some(neutral).filter(|n| !n.is_empty())),
        None => ("1", "0", None),
    };
    FieldWidget::CheckButton {
        on: on.to_owned(),
        off: off.to_owned(),
        neutral: neutral.filter(|_| group.is_none()).map(str::to_owned),
        group,
    }
}

fn choice(node: &XfaNode) -> FieldWidget {
    let (display, save) = choice_items(node);
    let w = widget(node);
    FieldWidget::ChoiceList {
        items: display
            .iter()
            .zip(&save)
            .map(|(d, s)| ((*d).to_owned(), (*s).to_owned()))
            .collect(),
        open: ChoiceOpen::parse(w.and_then(|w| w.attr("open"))),
        text_entry: w
            .and_then(|w| w.attr("textEntry"))
            .is_some_and(|t| t.trim() == "1"),
    }
}

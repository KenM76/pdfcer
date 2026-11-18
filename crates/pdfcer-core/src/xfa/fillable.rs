//! Planning the AcroForm fields a materialized XFA form keeps
//! ([`FieldOutput::Fillable`](super::FieldOutput::Fillable); pdfcer-gui
//! request G301). Field dictionaries: ISO 32000-1 §12.7.3 (names
//! §12.7.3.2, `/Ff` Tables 221, 226, 228, 230); XFA access p.605, presence
//! p.68, nullTest p.866.

use std::collections::HashMap;

use super::paint::{PaintedWidget, plain_text, widget};
use super::{BoundValue, ChoiceOpen, ContainerKind, FieldWidget, LayoutPage, Template, XfaNode};
use super::{field_widget, widget::widget_tag};

/// Something a fillable field could not carry from its XFA field.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct XfaFieldNote {
    /// The field's template path.
    pub path: Vec<usize>,
    /// What was not carried.
    pub kind: XfaFieldNoteKind,
}

/// The kinds of [`XfaFieldNote`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum XfaFieldNoteKind {
    /// A `calculate`, `validate` script or `event`: the written field does
    /// not run it.
    Script,
    /// A display, edit or validate picture clause: the field takes and
    /// shows its value as saved.
    Picture,
    /// A widget with no AcroForm equivalent here (`imageEdit`, `button`,
    /// `barcode`), by element name: drawn on the page, not fillable.
    Flat(String),
    /// A field split across pages or content areas: drawn on the page.
    Split,
    /// A check button in its neutral state: written off.
    Neutral,
}

/// What a planned field is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FillKind {
    Text,
    Check,
    Radio,
    Choice,
    Signature,
}

/// A planned field's value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FillValue {
    None,
    Text(String),
    Name(String),
    List(Vec<String>),
}

/// One terminal field and its widgets.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FillField {
    /// Partial names, outermost first (§12.7.3.2).
    pub(crate) name: Vec<String>,
    pub(crate) kind: FillKind,
    pub(crate) flags: u32,
    pub(crate) value: FillValue,
    pub(crate) max_len: Option<usize>,
    /// `(display, save)` for a choice.
    pub(crate) options: Vec<(String, String)>,
    /// `(page, widget index in that page's widgets, on state)`.
    pub(crate) widgets: Vec<(usize, usize, Option<String>)>,
}

const READ_ONLY: u32 = 1;
const REQUIRED: u32 = 1 << 1;
const MULTILINE: u32 = 1 << 12;
const PASSWORD: u32 = 1 << 13;
const NO_TOGGLE_TO_OFF: u32 = 1 << 14;
const RADIO: u32 = 1 << 15;
const COMBO: u32 = 1 << 17;
const EDIT: u32 = 1 << 18;
const MULTI_SELECT: u32 = 1 << 21;
const COMB: u32 = 1 << 24;

/// The fields `widgets` (one list per page of `pages`) become, and what
/// they could not carry. Fields bound to one data node in `bound` share a
/// field, so typing in one shows in all.
pub(crate) fn plan(
    template: &Template,
    pages: &[LayoutPage],
    widgets: &[&[PaintedWidget]],
    bound: &[BoundValue],
) -> (Vec<FillField>, Vec<XfaFieldNote>) {
    let mut notes = unwritten(template, pages);
    let data: HashMap<&[usize], &[usize]> = bound
        .iter()
        .map(|b| (b.path.as_slice(), b.data.as_slice()))
        .collect();
    let mut first_for_data: HashMap<&[usize], usize> = HashMap::new();
    let mut by_key: HashMap<Vec<usize>, usize> = HashMap::new();
    let mut fields: Vec<FillField> = Vec::new();
    for (page, list) in widgets.iter().enumerate() {
        for (i, w) in list.iter().enumerate() {
            let Some(fw) = field_widget(template, &w.path) else {
                continue;
            };
            let key = match &fw {
                FieldWidget::CheckButton { group: Some(g), .. } => g.clone(),
                _ => w.path.clone(),
            };
            let on = match &fw {
                FieldWidget::CheckButton { on, .. } => Some(on.clone()),
                _ => None,
            };
            let slot = (page, i, on);
            if let Some(f) = by_key.get(&key).and_then(|&f| fields.get_mut(f)) {
                f.widgets.push(slot);
                continue;
            }
            let Some(mut field) = field_of(template, &key, &w.path, &fw, &mut notes) else {
                continue;
            };
            let shared = data
                .get(key.as_slice())
                .and_then(|d| first_for_data.get(d).copied())
                .filter(|&f| {
                    field.kind != FillKind::Radio
                        && fields.get(f).is_some_and(|g| g.kind == field.kind)
                });
            if let Some(f) = shared
                && let Some(g) = fields.get_mut(f)
            {
                g.widgets.push(slot);
                by_key.insert(key, f);
                continue;
            }
            field.widgets.push(slot);
            if let Some(d) = data.get(key.as_slice()) {
                first_for_data.entry(d).or_insert(fields.len());
            }
            by_key.insert(key, fields.len());
            fields.push(field);
        }
    }
    radio_values(template, widgets, &mut fields);
    notes.dedup();
    (fields, notes)
}

/// Notes for laid-out fields that stay on the page.
fn unwritten(template: &Template, pages: &[LayoutPage]) -> Vec<XfaFieldNote> {
    let mut notes = Vec::new();
    for item in pages.iter().flat_map(|p| &p.items) {
        if item.kind != ContainerKind::Field {
            continue;
        }
        let Some(node) = template.node_at(&item.path) else {
            continue;
        };
        let kind = if item.fragment.is_some() {
            XfaFieldNoteKind::Split
        } else {
            match widget_tag(node) {
                tag @ ("imageEdit" | "button" | "barcode") => XfaFieldNoteKind::Flat(tag.into()),
                _ => continue,
            }
        };
        let note = XfaFieldNote {
            path: item.path.clone(),
            kind,
        };
        if !notes.contains(&note) {
            notes.push(note);
        }
    }
    notes
}

/// The field for the `field` (or radio group) at `key`, first seen through
/// the widget of the field at `path`.
fn field_of(
    template: &Template,
    key: &[usize],
    path: &[usize],
    fw: &FieldWidget,
    notes: &mut Vec<XfaFieldNote>,
) -> Option<FillField> {
    let node = template.node_at(path)?;
    let owner = template.node_at(key)?;
    let mut flags = access_flags(template, key) | required(owner);
    let value = plain_text(owner).unwrap_or("").to_owned();
    let mut max_len = None;
    let mut options = Vec::new();
    let (kind, value) = match fw {
        FieldWidget::Text {
            multiline,
            max_chars,
        } => {
            max_len = *max_chars;
            if let Some(cells) = comb_cells(node) {
                flags |= COMB;
                max_len = Some(cells);
            } else if *multiline {
                flags |= MULTILINE;
            }
            (FillKind::Text, text_value(value))
        }
        FieldWidget::Numeric | FieldWidget::Date => (FillKind::Text, text_value(value)),
        FieldWidget::Other(tag) if tag == "passwordEdit" => {
            flags |= PASSWORD;
            (FillKind::Text, text_value(value))
        }
        FieldWidget::Other(tag) if tag == "signature" => (FillKind::Signature, FillValue::None),
        FieldWidget::CheckButton {
            on, neutral, group, ..
        } => {
            if group.is_some() {
                flags |= RADIO | NO_TOGGLE_TO_OFF;
                (FillKind::Radio, FillValue::Name("Off".into()))
            } else {
                if neutral.as_deref().is_some_and(|n| n == value) {
                    notes.push(note(path, XfaFieldNoteKind::Neutral));
                }
                (FillKind::Check, check_state(&value, on))
            }
        }
        FieldWidget::ChoiceList {
            items,
            open,
            text_entry,
        } => {
            options.clone_from(items);
            let (more, value) = choice_value(value, open, *text_entry);
            flags |= more;
            (FillKind::Choice, value)
        }
        FieldWidget::Other(_) => return None,
    };
    carried_notes(node, path, notes);
    Some(FillField {
        name: partial_names(template, key),
        kind,
        flags,
        value,
        max_len,
        options,
        widgets: Vec::new(),
    })
}

fn note(path: &[usize], kind: XfaFieldNoteKind) -> XfaFieldNote {
    XfaFieldNote {
        path: path.to_vec(),
        kind,
    }
}

fn check_state(value: &str, on: &str) -> FillValue {
    FillValue::Name(if value == on {
        on.to_owned()
    } else {
        "Off".into()
    })
}

/// A choice list's extra `/Ff` bits and value.
fn choice_value(value: String, open: &ChoiceOpen, text_entry: bool) -> (u32, FillValue) {
    match open {
        ChoiceOpen::MultiSelect => (
            MULTI_SELECT,
            FillValue::List(value.lines().map(str::to_owned).collect()),
        ),
        ChoiceOpen::Always => (0, text_value(value)),
        _ => (COMBO | if text_entry { EDIT } else { 0 }, text_value(value)),
    }
}

fn text_value(v: String) -> FillValue {
    if v.is_empty() {
        FillValue::None
    } else {
        FillValue::Text(v)
    }
}

/// A `comb` text widget's cell count (`numberOfCells`, else `maxChars`).
fn comb_cells(node: &XfaNode) -> Option<usize> {
    let comb = widget(node)?.child("comb")?;
    comb.attr("numberOfCells")
        .and_then(|n| n.trim().parse().ok())
        .filter(|&n: &usize| n > 0)
        .or_else(|| {
            node.child("value")?
                .child("text")?
                .attr("maxChars")?
                .trim()
                .parse()
                .ok()
        })
}

/// Read-only when the container or an ancestor is not open to entry
/// (`access`, p.605).
fn access_flags(template: &Template, path: &[usize]) -> u32 {
    let mut node = template.element();
    for &i in path {
        let Some(n) = node.children.get(i) else {
            break;
        };
        node = n;
        if matches!(
            node.attr("access").map(str::trim),
            Some("readOnly" | "protected" | "nonInteractive")
        ) {
            return READ_ONLY;
        }
    }
    0
}

fn required(node: &XfaNode) -> u32 {
    let null_test = node.child("validate").and_then(|v| v.attr("nullTest"));
    if null_test.map(str::trim) == Some("error") {
        REQUIRED
    } else {
        0
    }
}

fn carried_notes(node: &XfaNode, path: &[usize], notes: &mut Vec<XfaFieldNote>) {
    let scripted = node.child("calculate").is_some()
        || node.child("event").is_some()
        || node
            .child("validate")
            .and_then(|v| v.child("script"))
            .is_some();
    if scripted {
        notes.push(note(path, XfaFieldNoteKind::Script));
    }
    let pictured = node
        .child("format")
        .and_then(|f| f.child("picture"))
        .is_some()
        || node
            .child("validate")
            .and_then(|v| v.child("picture"))
            .is_some()
        || node.child("ui").and_then(|u| u.child("picture")).is_some();
    if pictured {
        notes.push(note(path, XfaFieldNoteKind::Picture));
    }
}

/// The SOM-style partial names down to `path`: `name[k]` for each named
/// subform, exclusion group and field, `k` counting earlier same-named
/// siblings; an unnamed field or group is `#field[k]` / `#exclGroup[k]`.
pub(crate) fn partial_names(template: &Template, path: &[usize]) -> Vec<String> {
    let mut parts = Vec::new();
    let mut parent = template.element();
    for (depth, &i) in path.iter().enumerate() {
        let Some(node) = parent.children.get(i) else {
            break;
        };
        let earlier = parent.children.get(..i).unwrap_or_default();
        let last = depth + 1 == path.len();
        if matches!(node.tag.as_str(), "subform" | "exclGroup" | "field") {
            match node.attr("name") {
                Some(name) => {
                    let k = earlier
                        .iter()
                        .filter(|c| c.attr("name") == Some(name))
                        .count();
                    parts.push(format!("{name}[{k}]"));
                }
                None if last => {
                    let k = earlier
                        .iter()
                        .filter(|c| c.tag == node.tag && c.attr("name").is_none())
                        .count();
                    parts.push(format!("#{}[{k}]", node.tag));
                }
                None => {}
            }
        }
        parent = node;
    }
    parts
}

/// A radio group's value: the on state of the member whose value is its
/// on value (p.715).
fn radio_values(template: &Template, widgets: &[&[PaintedWidget]], fields: &mut [FillField]) {
    for f in fields.iter_mut().filter(|f| f.kind == FillKind::Radio) {
        let selected = f.widgets.iter().find_map(|(page, i, on)| {
            let w = widgets.get(*page)?.get(*i)?;
            let value = template.node_at(&w.path).and_then(plain_text)?;
            on.as_deref().filter(|on| *on == value).map(str::to_owned)
        });
        if let Some(on) = selected {
            f.value = FillValue::Name(on);
        }
    }
}

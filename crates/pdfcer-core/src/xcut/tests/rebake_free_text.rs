//! 781.2.c: a `/FreeText` restyle under `redraw_as_plain` writes back no key
//! the source lacked unless it changed that property, keeps a callout's
//! `/IT`, `/CL`, `/LE` and `/RD`, draws the callout line and its `/LE`
//! ending (ISO 32000-2 §12.5.6.6, Table 179), and names what it cannot draw.

use super::fixture;
use crate::annot_author::Color;
use crate::edit::{EditSession, TextAnnotStyle, TextAnnotStyleChange};
use crate::editable::EditableSource;
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::xcut::ObjectKind;

fn dict(s: &EditSession, id: ObjId) -> Dict {
    match s.value(id) {
        Some(Object::Dict(d)) => d.clone(),
        other => panic!("{id:?} is not a dictionary: {other:?}"),
    }
}

fn key_set(s: &EditSession, id: ObjId) -> Vec<String> {
    let mut keys: Vec<String> = dict(s, id)
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(k.as_bytes()).into_owned())
        .collect();
    keys.sort();
    keys
}

fn appearance(s: &EditSession, id: ObjId) -> String {
    let Some(Object::Dict(ap)) = dict(s, id).get(b"AP").map(|o| s.resolve(o)).cloned() else {
        panic!("{id:?} has no /AP");
    };
    let Some(Object::Stream(stream)) = ap.get(b"N").map(|o| s.resolve(o)) else {
        panic!("{id:?} has no /AP /N stream");
    };
    String::from_utf8_lossy(s.stream_bytes(stream).unwrap_or_default()).into_owned()
}

fn restyle(s: &mut EditSession, id: ObjId, style: TextAnnotStyle) -> TextAnnotStyleChange {
    let style = TextAnnotStyle {
        redraw_as_plain: true,
        ..style
    };
    s.set_text_annot_style(id, &style)
        .unwrap_or_else(|e| panic!("restyle refused: {e}"))
}

fn colour() -> TextAnnotStyle {
    TextAnnotStyle {
        color: Some(Color::Rgb(0.0, 0.0, 1.0)),
        ..TextAnnotStyle::default()
    }
}

/// A colour-only restyle of a foreign-drawn text box and callout keeps the
/// source's key set exactly: no `/BS` beside its `/Border`, and the callout
/// keys as written.
#[test]
fn a_colour_restyle_keeps_the_key_set() {
    for kind in [ObjectKind::FreeText, ObjectKind::Callout] {
        let (mut s, id) = fixture::build(kind);
        let before = dict(&s, id);
        let change = restyle(&mut s, id, colour());
        assert_eq!(key_set(&s, id), {
            let (s0, _) = fixture::build(kind);
            key_set(&s0, id)
        });
        let after = dict(&s, id);
        for key in [&b"Border"[..], b"IT", b"CL", b"LE", b"RD"] {
            assert_eq!(after.get(key), before.get(key), "{kind:?} /{key:?}");
        }
        assert!(!change.border_style_added, "{kind:?}");
        assert!(change.keys_not_drawn.is_empty(), "{kind:?}");
    }
}

/// A width restyle patches the width where the source holds it (`/Border`'s
/// third element, Table 164), and adds `/BS` only to a source with neither,
/// which the outcome reports.
#[test]
fn a_width_restyle_patches_the_source_border() {
    let width = TextAnnotStyle {
        border_width: Some(3.0),
        ..TextAnnotStyle::default()
    };
    let (mut s, id) = fixture::build(ObjectKind::FreeText);
    let before = key_set(&s, id);
    let change = restyle(&mut s, id, width.clone());
    assert_eq!(key_set(&s, id), before);
    let border = dict(&s, id).get(b"Border").cloned();
    let third = border
        .as_ref()
        .and_then(Object::as_array)
        .and_then(|a| a.get(2))
        .and_then(Object::as_number);
    assert_eq!(third, Some(3.0), "{border:?}");
    assert!(!change.border_style_added);

    let (mut s, id) = fixture::build_with_border(ObjectKind::FreeText, "");
    let mut expected = key_set(&s, id);
    expected.push("BS".to_owned());
    expected.sort();
    let change = restyle(&mut s, id, width);
    assert_eq!(key_set(&s, id), expected);
    assert!(change.border_style_added);
}

/// Removing the border (width 0) of a `/Border`-only box writes the 0 there
/// rather than leaving the dictionary claiming the old width.
#[test]
fn a_border_removal_is_written_where_the_source_holds_it() {
    let (mut s, id) = fixture::build(ObjectKind::FreeText);
    let mut expected = key_set(&s, id);
    expected.retain(|k| k != "C");
    restyle(
        &mut s,
        id,
        TextAnnotStyle {
            border_width: Some(0.0),
            ..TextAnnotStyle::default()
        },
    );
    assert_eq!(key_set(&s, id), expected);
    let third = dict(&s, id)
        .get(b"Border")
        .and_then(Object::as_array)
        .and_then(|a| a.get(2))
        .and_then(Object::as_number);
    assert_eq!(third, Some(0.0));
}

/// The redrawn callout draws its line from `/CL`'s start through the knee,
/// the `/OpenArrow` barbs meeting at the start (the tip is `0 0` in the
/// appearance box, `/Rect`'s corner), and the text box inside `/RD`.
#[test]
fn a_redrawn_callout_draws_its_line_and_arrowhead() {
    let (mut s, id) = fixture::build(ObjectKind::Callout);
    restyle(&mut s, id, colour());
    let ap = appearance(&s, id);
    let lines: Vec<&str> = ap.lines().map(str::trim).collect();
    let line_at = |op: &str| lines.iter().position(|l| *l == op);
    let start = line_at("0 0 m").unwrap_or_else(|| panic!("no callout start:\n{ap}"));
    assert_eq!(lines.get(start + 1), Some(&"20 20 l"), "{ap}");
    assert_eq!(lines.get(start + 2), Some(&"50 20 l"), "{ap}");
    assert!(line_at("0 0 l").is_some(), "no arrowhead tip:\n{ap}");
    assert!(
        line_at("1 0 0 1 5 5 cm").is_some(),
        "text box not inset:\n{ap}"
    );
}

fn callout_with(le: &str) -> (EditSession, ObjId) {
    fixture::build_with_border(
        ObjectKind::FreeText,
        &format!(
            "/Border [0 0 2] /IT /FreeTextCallout /CL [100 100 150 120] /LE /{le}              /RD [5 5 5 5] /BE << /S /C /I 1 >>"
        ),
    )
}

/// Every Table 179 style is drawn, so none is reported; the cloudy `/BE`
/// (drawn straight) still is, and the dictionary keeps both.
#[test]
fn every_table_179_callout_ending_is_drawn_not_reported() {
    for ending in crate::annot_author::LineEnding::ALL {
        let le = String::from_utf8_lossy(ending.name()).into_owned();
        let (mut s, id) = callout_with(&le);
        let before = key_set(&s, id);
        let change = restyle(&mut s, id, colour());
        assert_eq!(key_set(&s, id), before, "/{le}");
        assert_eq!(change.keys_not_drawn, vec!["BE".to_owned()], "/{le}");
    }
}

/// A callout ending outside Table 179 and a cloudy `/BE` stay in the
/// dictionary and are named in the outcome, never dropped silently.
#[test]
fn an_undrawable_callout_ending_is_kept_and_reported() {
    let (mut s, id) = callout_with("Arrow");
    let before = key_set(&s, id);
    let change = restyle(&mut s, id, colour());
    assert_eq!(key_set(&s, id), before);
    assert!(matches!(dict(&s, id).get(b"LE"), Some(Object::Name(n)) if n.as_bytes() == b"Arrow"));
    assert_eq!(
        change.keys_not_drawn,
        vec!["LE".to_owned(), "BE".to_owned()]
    );
}

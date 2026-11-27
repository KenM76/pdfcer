//! 781.2.b: every markup re-bake (restyle, reshape, respan, paste) writes
//! back no key the source lacked unless it changed that property, and reads
//! a dash from `/Border`'s fourth element (ISO 32000-1 §12.5.2 Table 164).

use super::fixture;
use super::runner::{annots, copy};
use crate::annot_author::{Color, Quad};
use crate::edit::{EditSession, MarkupStyle, MarkupStyleSupport, StyleEdit, VertexEdit};
use crate::editable::EditableSource;
use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::page_tree::Rect;
use crate::vector::Matrix;
use crate::xcut::policy::clipboard::MARKUP;
use crate::xcut::{Change, Expect, ObjectKind, Op, expect};

/// The markup kinds the clipboard policy re-bakes.
fn markup_kinds() -> impl Iterator<Item = ObjectKind> {
    fixture::kinds().filter(|k| matches!(expect(Op::Paste, *k), Expect::Preserve(c) if c == MARKUP))
}

/// `id`'s key set less the keys a paste declares it drops.
fn pasted_key_set(s: &EditSession, id: ObjId) -> Vec<String> {
    let mut keys = key_set(s, id);
    keys.retain(|k| {
        !MARKUP
            .iter()
            .any(|c| matches!(c, Change::Dropped { key, .. } if *key == k.as_str()))
    });
    keys
}

fn dict(s: &EditSession, id: ObjId) -> crate::object::Dict {
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

fn subtype(s: &EditSession, id: ObjId) -> Vec<u8> {
    match dict(s, id).get(b"Subtype") {
        Some(Object::Name(n)) => n.as_bytes().to_vec(),
        _ => Vec::new(),
    }
}

/// Whether `id`'s normal appearance sets a dash (`d`, §8.4.3.6); pdfcer
/// emits none for a solid border.
fn appearance_has_dash(s: &EditSession, id: ObjId) -> bool {
    let Some(Object::Dict(ap)) = dict(s, id).get(b"AP").map(|o| s.resolve(o)).cloned() else {
        return false;
    };
    let Some(Object::Stream(stream)) = ap.get(b"N").map(|o| s.resolve(o)) else {
        return false;
    };
    let text = String::from_utf8_lossy(s.stream_bytes(stream).unwrap_or_default()).into_owned();
    text.contains(" d\n") || text.contains(" d ")
}

fn border_dash(s: &EditSession, id: ObjId) -> Option<Vec<f64>> {
    let page = super::runner::pages(s)[0];
    crate::annot::page_annotations(s, page)
        .into_iter()
        .find(|a| a.id == Some(id))
        .and_then(|a| a.border_dash)
        .map(|d| d.pattern().to_vec())
}

/// A colour-only, opacity-only and width-only restyle of every markup kind
/// keeps the source's key set: a Highlight gains no `/BM` (ISO 32000-2
/// §12.5.2 Table 166: absent is Normal; its appearance selects Multiply),
/// and a width held in `/Border` is patched there (Table 164), not moved
/// into a new `/BS`.
#[test]
#[allow(clippy::unwrap_used)]
fn a_restyle_writes_only_the_keys_it_names() {
    let mut faults = Vec::new();
    for kind in markup_kinds() {
        let styles = [
            (
                "colour",
                MarkupStyle {
                    stroke: Some(StyleEdit::Set(Color::Rgb(0.0, 0.5, 0.0))),
                    ..Default::default()
                },
            ),
            (
                "opacity",
                MarkupStyle {
                    opacity: Some(StyleEdit::Set(0.25)),
                    ..Default::default()
                },
            ),
            (
                "width",
                MarkupStyle {
                    width: Some(4.0),
                    ..Default::default()
                },
            ),
        ];
        for (label, style) in styles {
            let (mut s, id) = fixture::build(kind);
            if label == "width" && !MarkupStyleSupport::for_subtype(&subtype(&s, id)).takes_border {
                continue;
            }
            let want = key_set(&s, id);
            if let Err(e) = s.set_markup_style(id, &style) {
                faults.push(format!("{kind:?} {label}: {e}"));
                continue;
            }
            let got = key_set(&s, id);
            if got != want {
                faults.push(format!("{kind:?} {label}: keys {got:?}, want {want:?}"));
            }
            if label == "width" {
                let width = super::runner::dump(&s, id).0.get("/Border[2]").cloned();
                if width != Some(crate::xcut::dump::Canon::Num(4.0)) {
                    faults.push(format!("{kind:?} width: /Border[2] {width:?}, want 4"));
                }
            }
        }
    }
    assert!(faults.is_empty(), "{faults:#?}");
}

/// A geometry re-bake (node drag, text re-span) writes no style key either.
#[test]
#[allow(clippy::unwrap_used)]
fn a_reshape_and_a_respan_add_no_key() {
    let (mut s, id) = fixture::build(ObjectKind::Polygon);
    let want = key_set(&s, id);
    s.reshape_annotation(
        id,
        VertexEdit::Move {
            index: 0,
            dx: 5.0,
            dy: 5.0,
        },
        None,
    )
    .unwrap();
    assert_eq!(key_set(&s, id), want, "reshape");

    let (mut s, id) = fixture::build(ObjectKind::Highlight);
    let want = key_set(&s, id);
    let quad = Quad::from_rect(Rect::from_corners(100.0, 100.0, 180.0, 150.0));
    s.respan_text_markup(id, &[quad], None).unwrap();
    assert_eq!(key_set(&s, id), want, "respan");
}

/// A resize of every markup kind re-bakes without adding a style key.
#[test]
#[allow(clippy::unwrap_used)]
fn a_resize_adds_no_key() {
    let mut faults = Vec::new();
    for kind in markup_kinds() {
        let (mut s, id) = fixture::build(kind);
        // The fixture's `/F 28` sets NoZoom, which resize refuses (Table 165
        // bit 4); Print alone keeps the key and lifts the refusal.
        let mut target = dict(&s, id);
        target.insert(crate::object::Name::from(b"F"), Object::Integer(4));
        s.xcut_put_object(id, Object::Dict(target));
        // Resize re-bakes only an appearance pdfcer drew; a restyle draws it.
        let redraw = MarkupStyle {
            opacity: Some(StyleEdit::Set(0.5)),
            ..Default::default()
        };
        s.set_markup_style(id, &redraw).unwrap();
        let want = key_set(&s, id);
        let opts = crate::edit::ResizeOptions::default();
        if let Err(e) = s.resize_annotation(id, (100.0, 100.0), 1.5, 1.5, &opts) {
            faults.push(format!("{kind:?}: {e}"));
            continue;
        }
        let got = key_set(&s, id);
        if got != want {
            faults.push(format!("{kind:?}: keys {got:?}, want {want:?}"));
        }
    }
    assert!(faults.is_empty(), "{faults:#?}");
}

/// Table 164: `/Border [0 0 2 [3 2]]` with no `/BS` is a 3-on 2-off dash.
/// It is read, survives a rotated paste (which re-bakes) without moving into
/// a new `/BS`, and survives a colour restyle.
#[test]
#[allow(clippy::unwrap_used)]
fn a_border_array_dash_is_read_and_kept() {
    let quarter = Matrix::new(0.0, 1.0, -1.0, 0.0, 400.0, 0.0);
    let (mut s, id) = fixture::build_with_border(ObjectKind::Square, "/Border [0 0 2 [3 2]]");
    assert_eq!(border_dash(&s, id), Some(vec![3.0, 2.0]), "read");
    let want = key_set(&s, id);
    let want_pasted = pasted_key_set(&s, id);

    let clip = copy(&s, id).unwrap();
    s.paste_objects(0, &clip, quarter).unwrap();
    let pasted = *annots(&s, 0).last().unwrap();
    assert!(
        appearance_has_dash(&s, pasted),
        "pasted appearance is solid"
    );
    assert_eq!(border_dash(&s, pasted), Some(vec![3.0, 2.0]), "pasted");
    assert_eq!(key_set(&s, pasted), want_pasted, "paste keys");

    s.set_markup_style(
        id,
        &MarkupStyle {
            stroke: Some(StyleEdit::Set(Color::Rgb(0.0, 0.0, 1.0))),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(appearance_has_dash(&s, id), "restyled appearance is solid");
    assert_eq!(border_dash(&s, id), Some(vec![3.0, 2.0]), "restyled");
    assert_eq!(key_set(&s, id), want, "restyle keys");
}

/// A mark with neither `/Border` nor `/BS` (ISO 32000-1 §12.5.4: solid, 1pt)
/// pasted at twice the size needs a width it has nowhere to hold: the
/// border-taking kinds gain `/BS` and the paste says so; a text markup,
/// which draws no border, gains nothing.
#[test]
#[allow(clippy::unwrap_used)]
fn a_scaled_paste_of_a_borderless_mark_discloses_its_added_border_style() {
    let turn_and_grow = Matrix::new(0.0, 2.0, -2.0, 0.0, 500.0, 0.0);
    let mut faults = Vec::new();
    for kind in markup_kinds() {
        let (mut s, id) = fixture::build_with_border(kind, "");
        let mut want = pasted_key_set(&s, id);
        assert!(!want.iter().any(|k| k == "Border" || k == "BS"), "{kind:?}");
        let takes_border = MarkupStyleSupport::for_subtype(&subtype(&s, id)).takes_border;
        if takes_border {
            want.push("BS".to_owned());
            want.sort();
        }
        let clip = copy(&s, id).unwrap();
        let outcome = s.paste_objects(0, &clip, turn_and_grow).unwrap();
        let pasted = *annots(&s, 0).last().unwrap();
        let got = key_set(&s, pasted);
        if got != want {
            faults.push(format!("{kind:?}: keys {got:?}, want {want:?}"));
        }
        let disclosed = outcome
            .disclosures
            .iter()
            .any(|d| d.contains("ADDED a /BS"));
        if disclosed != takes_border {
            faults.push(format!(
                "{kind:?}: /BS disclosure {disclosed}, want {takes_border}: {:?}",
                outcome.disclosures
            ));
        }
    }
    assert!(faults.is_empty(), "{faults:#?}");
}

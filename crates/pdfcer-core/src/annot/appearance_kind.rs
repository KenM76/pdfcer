//! Selecting an annotation's rollover or down appearance (ISO 32000-1
//! §12.5.5 Table 168) instead of its normal one.

use super::{Appearance, select_ap_entry};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::settings::MissingAppearanceState;

/// Which of an annotation's three appearances to draw (ISO 32000-1 §12.5.5
/// Table 168): `/N`, `/R` or `/D`.
///
/// Each is a stream or an `/AS`-keyed state subdictionary, selected the same
/// way. `/R` and `/D` are optional and default to `/N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum AppearanceKind {
    /// `/N`, the appearance shown when the pointer is not over the
    /// annotation. What a page renders unless asked otherwise.
    #[default]
    Normal,
    /// `/R`, shown while the pointer is over the annotation.
    Rollover,
    /// `/D`, shown while the mouse button is held down within it.
    Down,
}

impl AppearanceKind {
    /// The `/AP` key this kind selects: `b"N"`, `b"R"` or `b"D"`.
    #[must_use]
    pub const fn ap_key(self) -> &'static [u8] {
        match self {
            Self::Normal => b"N",
            Self::Rollover => b"R",
            Self::Down => b"D",
        }
    }
}

/// The outcome of [`select_appearance`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SelectedAppearance {
    /// The appearance to draw, in the same taxonomy as
    /// [`Annotation::appearance`](super::Annotation::appearance).
    pub appearance: Appearance,
    /// The requested `/R` or `/D` was absent or yielded no stream, so this is
    /// the `/N` appearance (Table 168: "Default value: the value of the N
    /// entry"). Always `false` for [`AppearanceKind::Normal`]. A caller
    /// discloses it: the operator asked for a state the file does not draw.
    pub fell_back_to_normal: bool,
    /// [`AppearanceKind::Down`] was asked of a widget whose `/H` is not `P`
    /// or `T` (absent means `I`). Table 188: "A highlighting mode other than
    /// P shall override any down appearance", so a conforming viewer would
    /// not show `/D` when pressed. The selection still honours the request.
    pub highlight_overrides_down: bool,
}

/// Select `annot`'s appearance of `kind` (§12.5.5): the `/AP` entry
/// [`AppearanceKind::ap_key`] names, a stream or an `/AS`-keyed state
/// subdictionary resolved exactly as `/N` is, with `missing_as` deciding a
/// subdictionary with no `/AS`.
///
/// When that entry is absent, or resolves to no stream, the `/N` appearance
/// is returned and [`SelectedAppearance::fell_back_to_normal`] is set. Never
/// synthesises an appearance; a widget with no `/AP` at all comes back as
/// [`Appearance::None`] for the caller to build one.
#[must_use]
pub fn select_appearance<G: ObjectGraph + ?Sized>(
    graph: &G,
    annot: &Dict,
    kind: AppearanceKind,
    missing_as: MissingAppearanceState,
) -> SelectedAppearance {
    let chosen = select_ap_entry(graph, annot, kind.ap_key(), missing_as);
    let normal = || select_ap_entry(graph, annot, b"N", missing_as);
    let (appearance, fell_back_to_normal) = match (kind, chosen) {
        (AppearanceKind::Normal, a) => (a, false),
        (_, a @ Appearance::Normal { .. }) => (a, false),
        _ => (normal(), true),
    };
    SelectedAppearance {
        appearance,
        fell_back_to_normal,
        highlight_overrides_down: kind == AppearanceKind::Down && down_is_overridden(graph, annot),
    }
}

/// Whether a widget's `/H` (Table 188) overrides its down appearance.
fn down_is_overridden<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict) -> bool {
    let is_widget = annot
        .get(b"Subtype")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == b"Widget");
    let mode = annot
        .get(b"H")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_name)
        .map_or(&b"I"[..], |n| n.as_bytes());
    is_widget && !matches!(mode, b"P" | b"T")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Name, ObjId, Stream};
    use crate::span::ByteSpan;
    use std::collections::BTreeMap;

    struct Graph(BTreeMap<ObjId, Object>);

    impl ObjectGraph for Graph {
        fn value(&self, id: ObjId) -> Option<&Object> {
            self.0.get(&id)
        }
        fn trailer_entry(&self, _key: &[u8]) -> Option<&Object> {
            None
        }
    }

    fn id(n: u32) -> ObjId {
        ObjId::new(n, 0)
    }

    fn graph() -> Graph {
        let stream = || {
            Object::Stream(Stream {
                dict: Dict::default(),
                data_span: ByteSpan { start: 0, len: 0 },
            })
        };
        Graph((1..=3).map(|n| (id(n), stream())).collect())
    }

    fn annot(entries: &[(&[u8], u32)], h: Option<&[u8]>) -> Dict {
        let mut ap = Dict::new();
        for &(k, n) in entries {
            ap.insert(Name::from(k), Object::Reference(id(n)));
        }
        let mut d = Dict::new();
        d.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Widget")));
        d.insert(Name::from(b"AP"), Object::Dict(ap));
        if let Some(h) = h {
            d.insert(Name::from(b"H"), Object::Name(Name::from(h)));
        }
        d
    }

    fn pick(d: &Dict, kind: AppearanceKind) -> SelectedAppearance {
        select_appearance(&graph(), d, kind, MissingAppearanceState::default())
    }

    #[test]
    fn each_kind_selects_its_own_stream() {
        let d = annot(&[(b"N", 1), (b"R", 2), (b"D", 3)], Some(b"P"));
        for (kind, n) in [
            (AppearanceKind::Normal, 1),
            (AppearanceKind::Rollover, 2),
            (AppearanceKind::Down, 3),
        ] {
            let got = pick(&d, kind);
            assert_eq!(
                got.appearance,
                Appearance::Normal {
                    stream_id: Some(id(n))
                }
            );
            assert!(!got.fell_back_to_normal && !got.highlight_overrides_down);
        }
    }

    #[test]
    fn a_missing_state_falls_back_to_normal_and_says_so() {
        let d = annot(&[(b"N", 1)], None);
        let got = pick(&d, AppearanceKind::Down);
        assert_eq!(
            got.appearance,
            Appearance::Normal {
                stream_id: Some(id(1))
            }
        );
        assert!(got.fell_back_to_normal);
        assert!(
            got.highlight_overrides_down,
            "absent /H is I, which overrides /D"
        );
        assert!(!pick(&d, AppearanceKind::Normal).fell_back_to_normal);
    }
}

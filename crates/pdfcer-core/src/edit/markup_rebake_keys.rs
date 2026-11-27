//! The key-set rule for every markup re-bake (paste, restyle, reshape,
//! respan, resize): the redraw replaces the appearance and the geometry, and
//! writes back no style key the source did not have unless the verb changed
//! that property (decision 228, clone-then-patch).

use super::EditSession;
use crate::annot_author::{self, BorderDash};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, Object};

/// Style keys a re-bake writes whose ABSENCE already means the value the
/// re-bake read, so writing them adds a key and changes nothing:
///
/// - `/BM` — ISO 32000-2 §12.5.2 Table 166: optional, default `/Normal`.
///   pdfcer's highlighter appearance selects Multiply in its own
///   `/ExtGState` (§11.3.5, §8.4.5), which is what an ISO 32000-1 reader
///   honours anyway (1.7 §12.5.5 composites the appearance with Normal).
///   Keeping the source's absent `/BM` keeps its outer compositing.
/// - `/C` — read with a default when absent.
/// - `/IC` — written only when read.
/// - `/LE` — Table 176: absent means `[/None /None]`, the value read.
const IMPLIED_WHEN_ABSENT: [&[u8]; 4] = [b"BM", b"C", b"IC", b"LE"];

/// Which style properties the re-baking verb itself changed. A changed
/// property's key is written even when the source lacked it; everything
/// else is patched back to the source.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Rewrites {
    /// `/C` was set or cleared.
    pub(super) stroke: bool,
    /// `/IC` was set or cleared.
    pub(super) interior: bool,
    /// `/LE` was set or cleared.
    pub(super) endings: bool,
    /// `/BE` was set or cleared.
    pub(super) border_effect: bool,
    /// The dash changed to this (`None` = solid). `None` here = unchanged.
    pub(super) dash: Option<Option<BorderDash>>,
}

impl Rewrites {
    fn writes(&self, key: &[u8]) -> bool {
        match key {
            b"C" => self.stroke,
            b"IC" => self.interior,
            b"LE" => self.endings,
            _ => false,
        }
    }
}

/// What [`keep_source_keys`] had to add.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AddedKey {
    /// Nothing: the border keys are the source's.
    None,
    /// `/BS`: the width or dash changed and the source held it in neither
    /// `/BS` nor a direct `/Border` array.
    BorderStyle,
}

/// Patch `updated` (a re-baked copy of `current`) back to `current`'s key
/// set and key order, keeping only what the redraw changed.
///
/// `source_width` is the width `current` implies (`/BS /W`, else `/Border`'s
/// third element, else 1 — ISO 32000-1 §12.5.4, Tables 164 and 166) and
/// `source_bs` its `/BS` resolved; `/BS /W` in `updated` is the redrawn
/// width. The border keys are restored from `current` and only a changed
/// width or [`Rewrites::dash`] is patched, where `current` holds it: in its
/// `/BS`, else in a direct `/Border` array (Table 164: width third, dash
/// fourth), else by adding `/BS`, which is the one addition and is returned
/// so a verb the operator did not aim at the border can disclose it.
pub(super) fn keep_source_keys(
    current: &Dict,
    source_bs: Option<&Dict>,
    updated: &mut Dict,
    source_width: f64,
    rewrites: &Rewrites,
) -> AddedKey {
    for key in IMPLIED_WHEN_ABSENT {
        if !current.contains_key(key) && !rewrites.writes(key) {
            updated.remove(key);
        }
    }
    if !rewrites.border_effect
        && let Some(be) = current.get(b"BE")
    {
        updated.insert(Name::from(b"BE"), be.clone());
    }
    let width = redrawn_width(updated).filter(|w| (w - source_width).abs() > 1e-9);
    updated.remove(b"BS");
    if let Some(bs) = current.get(b"BS") {
        updated.insert(Name::from(b"BS"), bs.clone());
    }
    let added = patch_border(current, source_bs, updated, width, rewrites.dash.as_ref());
    restore_order(current, updated);
    added
}

impl EditSession {
    /// [`keep_source_keys`] with `current`'s `/BS` and width read through
    /// the session graph.
    pub(super) fn keep_markup_source_keys(
        &self,
        current: &Dict,
        updated: &mut Dict,
        rewrites: &Rewrites,
    ) -> AddedKey {
        let graph = self.graph();
        let source_bs = match current.get(b"BS").map(|o| graph.resolve(o)) {
            Some(Object::Dict(bs)) => Some(bs.clone()),
            _ => None,
        };
        let width = annot_author::read_border_width(&graph, current);
        keep_source_keys(current, source_bs.as_ref(), updated, width, rewrites)
    }
}

fn redrawn_width(updated: &Dict) -> Option<f64> {
    match updated.get(b"BS") {
        Some(Object::Dict(bs)) => bs.get(b"W").and_then(Object::as_number),
        _ => None,
    }
}

fn patch_border(
    current: &Dict,
    source_bs: Option<&Dict>,
    updated: &mut Dict,
    width: Option<f64>,
    dash: Option<&Option<BorderDash>>,
) -> AddedKey {
    if width.is_none() && dash.is_none() {
        return AddedKey::None;
    }
    if let Some(bs) = source_bs {
        let mut bs = bs.clone();
        if let Some(w) = width {
            bs.insert(Name::from(b"W"), Object::Real(w));
        }
        if let Some(d) = dash {
            set_style_dash(&mut bs, d.as_ref());
        }
        updated.insert(Name::from(b"BS"), Object::Dict(bs));
        return AddedKey::None;
    }
    if let Some(Object::Array(border)) = current.get(b"Border")
        && border.len() >= 3
    {
        let mut border = border.clone();
        if let (Some(w), Some(slot)) = (width, border.get_mut(2)) {
            *slot = Object::Real(w);
        }
        if let Some(d) = dash {
            border.truncate(3);
            border.extend(d.as_ref().map(dash_array));
        }
        updated.insert(Name::from(b"Border"), Object::Array(border));
        return AddedKey::None;
    }
    let mut bs = Dict::new();
    if let Some(w) = width {
        bs.insert(Name::from(b"W"), Object::Real(w));
    }
    if let Some(Some(d)) = dash {
        set_style_dash(&mut bs, Some(d));
    }
    if bs.is_empty() {
        return AddedKey::None;
    }
    updated.insert(Name::from(b"BS"), Object::Dict(bs));
    AddedKey::BorderStyle
}

/// ISO 32000-1 Table 166: `/S /D` with its `/D` array; solid is `/S /S`
/// with no `/D`. A clear rewrites `/S` only where it said `/D`, so a source
/// with no `/S` (already solid) gains none.
fn set_style_dash(bs: &mut Dict, dash: Option<&BorderDash>) {
    match dash {
        Some(d) => {
            bs.insert(Name::from(b"S"), Object::Name(Name::from(b"D")));
            bs.insert(Name::from(b"D"), dash_array(d));
        }
        None => {
            if matches!(bs.get(b"S"), Some(Object::Name(n)) if n.as_bytes() == b"D") {
                bs.insert(Name::from(b"S"), Object::Name(Name::from(b"S")));
            }
            bs.remove(b"D");
        }
    }
}

fn dash_array(d: &BorderDash) -> Object {
    Object::Array(d.pattern().iter().copied().map(Object::Real).collect())
}

/// Reorder `updated` so `current`'s keys come first in `current`'s order; the
/// spec ignores order (§7.3.7), a byte diff does not.
fn restore_order(current: &Dict, updated: &mut Dict) {
    let rank = |k: &Name| {
        current
            .iter()
            .position(|(c, _)| c == k)
            .unwrap_or(usize::MAX)
    };
    updated.0.sort_by_key(|(k, _)| rank(k));
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)] // a panic here is the test failing
mod tests {
    use super::*;

    fn dict(entries: &[(&[u8], Object)]) -> Dict {
        let mut d = Dict::new();
        for (k, v) in entries {
            d.insert(Name::from(*k), v.clone());
        }
        d
    }

    fn bs(w: f64) -> Object {
        Object::Dict(dict(&[
            (b"Type", Object::Name(Name::from(b"Border"))),
            (b"W", Object::Real(w)),
        ]))
    }

    fn keys(d: &Dict) -> Vec<Vec<u8>> {
        d.iter().map(|(k, _)| k.as_bytes().to_vec()).collect()
    }

    #[test]
    fn an_unchanged_width_adds_no_border_style() {
        let current = dict(&[(b"Subtype", Object::Name(Name::from(b"Ink")))]);
        let mut updated = current.clone();
        updated.insert(Name::from(b"BS"), bs(1.0));
        updated.insert(Name::from(b"C"), Object::Array(vec![Object::Real(0.0)]));
        let added = keep_source_keys(&current, None, &mut updated, 1.0, &Rewrites::default());
        assert_eq!(added, AddedKey::None);
        assert_eq!(keys(&updated), keys(&current));
    }

    #[test]
    fn a_scaled_width_patches_the_border_array() {
        let border = Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(2),
        ]);
        let current = dict(&[(b"Border", border)]);
        let mut updated = current.clone();
        updated.insert(Name::from(b"BS"), bs(4.0));
        assert_eq!(
            keep_source_keys(&current, None, &mut updated, 2.0, &Rewrites::default()),
            AddedKey::None
        );
        assert_eq!(keys(&updated), keys(&current));
        let width = match updated.get(b"Border") {
            Some(Object::Array(b)) => b.get(2).and_then(Object::as_number),
            _ => None,
        };
        assert_eq!(width, Some(4.0), "{updated:?}");
    }

    #[test]
    fn a_scaled_width_patches_only_w_in_the_source_border_style() {
        let source_bs = Object::Dict(dict(&[
            (b"W", Object::Integer(1)),
            (b"S", Object::Name(Name::from(b"B"))),
        ]));
        let current = dict(&[(b"BS", source_bs)]);
        let resolved = current.get(b"BS").and_then(Object::as_dict).cloned();
        let mut updated = current.clone();
        updated.insert(Name::from(b"BS"), bs(2.0));
        let added = keep_source_keys(
            &current,
            resolved.as_ref(),
            &mut updated,
            1.0,
            &Rewrites::default(),
        );
        assert_eq!(added, AddedKey::None);
        let b = updated.get(b"BS").and_then(Object::as_dict).unwrap();
        assert_eq!(keys(b), vec![b"W".to_vec(), b"S".to_vec()]);
        assert_eq!(b.get(b"W").and_then(Object::as_number), Some(2.0));
    }

    #[test]
    fn a_scaled_width_with_nowhere_to_live_adds_and_reports_border_style() {
        let current = dict(&[(b"Subtype", Object::Name(Name::from(b"Ink")))]);
        let mut updated = current.clone();
        updated.insert(Name::from(b"BS"), bs(2.0));
        let added = keep_source_keys(&current, None, &mut updated, 1.0, &Rewrites::default());
        assert_eq!(added, AddedKey::BorderStyle);
        assert_eq!(keys(&updated), vec![b"Subtype".to_vec(), b"BS".to_vec()]);
    }

    #[test]
    fn a_source_blend_mode_is_kept_and_an_absent_one_stays_absent() {
        let multiply = Object::Name(Name::from(b"Multiply"));
        let with = dict(&[(b"BM", multiply.clone())]);
        let mut updated = with.clone();
        keep_source_keys(&with, None, &mut updated, 1.0, &Rewrites::default());
        assert_eq!(updated.get(b"BM"), Some(&multiply));
        let without = Dict::new();
        let mut updated = dict(&[(b"BM", multiply)]);
        keep_source_keys(&without, None, &mut updated, 1.0, &Rewrites::default());
        assert!(updated.get(b"BM").is_none());
    }

    fn border_array(d: &Dict) -> Vec<Object> {
        match d.get(b"Border") {
            Some(Object::Array(b)) => b.clone(),
            other => panic!("no /Border array: {other:?}"),
        }
    }

    /// Table 164: a dash change on a `/Border`-only source is written as the
    /// array's fourth element, and a clear truncates it; no `/BS` appears.
    #[test]
    fn a_dash_change_patches_the_border_array() {
        let solid = vec![Object::Integer(0), Object::Integer(0), Object::Integer(2)];
        let current = dict(&[(b"Border", Object::Array(solid.clone()))]);
        let mut updated = current.clone();
        let dashed = Rewrites {
            dash: Some(BorderDash::new(vec![3.0, 2.0])),
            ..Default::default()
        };
        let added = keep_source_keys(&current, None, &mut updated, 2.0, &dashed);
        assert_eq!(added, AddedKey::None);
        assert_eq!(keys(&updated), keys(&current));
        let pattern = Object::Array(vec![Object::Real(3.0), Object::Real(2.0)]);
        assert_eq!(border_array(&updated).get(3), Some(&pattern));

        let mut cleared = updated.clone();
        let solidify = Rewrites {
            dash: Some(None),
            ..Default::default()
        };
        keep_source_keys(&updated, None, &mut cleared, 2.0, &solidify);
        assert_eq!(border_array(&cleared), solid);
    }

    /// Table 166: solidifying a `/BS /S /D` source rewrites `/S` and drops
    /// `/D`; a `/BS` with no `/S` gains none.
    #[test]
    fn a_dash_clear_in_the_border_style_writes_solid_only_where_it_said_dashed() {
        let dashed = dict(&[
            (b"S", Object::Name(Name::from(b"D"))),
            (b"D", Object::Array(vec![Object::Real(3.0)])),
        ]);
        let current = dict(&[(b"BS", Object::Dict(dashed.clone()))]);
        let mut updated = current.clone();
        let clear = Rewrites {
            dash: Some(None),
            ..Default::default()
        };
        keep_source_keys(&current, Some(&dashed), &mut updated, 1.0, &clear);
        let Some(Object::Dict(got)) = updated.get(b"BS") else {
            panic!("{updated:?}");
        };
        assert_eq!(got.get(b"S"), Some(&Object::Name(Name::from(b"S"))));
        assert!(!got.contains_key(b"D"));

        let bare = dict(&[(b"D", Object::Array(vec![Object::Real(3.0)]))]);
        let current = dict(&[(b"BS", Object::Dict(bare.clone()))]);
        let mut updated = current.clone();
        keep_source_keys(&current, Some(&bare), &mut updated, 1.0, &clear);
        let Some(Object::Dict(got)) = updated.get(b"BS") else {
            panic!("{updated:?}");
        };
        assert!(got.is_empty(), "{got:?}");
    }
}

//! The key-set rule for a re-baked markup paste: the redraw replaces the
//! appearance and the geometry, and writes back no style key the source did
//! not have unless its value changed (decision 228, clone-then-patch).

use crate::object::{Dict, Name, Object};

/// Style keys a re-bake writes whose ABSENCE already means the value the
/// re-bake read, so writing them adds a key and changes nothing:
///
/// - `/BM` — ISO 32000-2 §12.5.2 Table 166: optional, default `/Normal`.
///   pdfcer's highlighter appearance selects Multiply in its own
///   `/ExtGState` (§11.3.5, §8.4.5), which is what an ISO 32000-1 reader
///   honours anyway (1.7 §12.5.5 composites the appearance with Normal).
///   Keeping the source's absent `/BM` keeps its outer compositing.
/// - `/C` — read with a default when absent, and the paste does not restyle.
/// - `/IC` — written only when read.
/// - `/LE` — Table 176: absent means `[/None /None]`, the value read.
const IMPLIED_WHEN_ABSENT: [&[u8]; 4] = [b"BM", b"C", b"IC", b"LE"];

/// What [`keep_source_keys`] had to add.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AddedKey {
    /// Nothing: the pasted key set is the source's.
    None,
    /// `/BS`: a scale changed the border width and the source held it in
    /// neither `/BS` nor a direct `/Border` array.
    BorderStyle,
}

/// Patch `updated` (a re-baked copy of `current`) back to `current`'s key
/// set and key order, keeping only what the redraw changed.
///
/// `source_width` is the width `current` implies (`/BS /W`, else `/Border`'s
/// third element, else 1 — ISO 32000-1 §12.5.4, Table 166) and
/// `source_bs` its `/BS` resolved; `/BS /W` in
/// `updated` is the redrawn width. The border keys are restored from
/// `current` and only the width is patched: in a direct `/BS`, else in a
/// direct `/Border` array, else by adding `/BS << /W w >>`, which is the one
/// addition and is returned so the paste can disclose it.
pub(super) fn keep_source_keys(
    current: &Dict,
    source_bs: Option<&Dict>,
    updated: &mut Dict,
    source_width: f64,
) -> AddedKey {
    for key in IMPLIED_WHEN_ABSENT {
        if !current.contains_key(key) {
            updated.remove(key);
        }
    }
    let width = redrawn_width(updated);
    updated.remove(b"BS");
    let mut added = AddedKey::None;
    if let Some(bs) = current.get(b"BS") {
        updated.insert(Name::from(b"BS"), bs.clone());
    }
    if let Some(w) = width.filter(|w| (w - source_width).abs() > 1e-9) {
        added = patch_width(current, source_bs, updated, w);
    }
    restore_order(current, updated);
    added
}

fn redrawn_width(updated: &Dict) -> Option<f64> {
    match updated.get(b"BS") {
        Some(Object::Dict(bs)) => bs.get(b"W").and_then(Object::as_number),
        _ => None,
    }
}

fn patch_width(current: &Dict, source_bs: Option<&Dict>, updated: &mut Dict, w: f64) -> AddedKey {
    if let Some(bs) = source_bs {
        let mut bs = bs.clone();
        bs.insert(Name::from(b"W"), Object::Real(w));
        updated.insert(Name::from(b"BS"), Object::Dict(bs));
        return AddedKey::None;
    }
    if let Some(Object::Array(border)) = current.get(b"Border") {
        let mut border = border.clone();
        if let Some(width) = border.get_mut(2) {
            *width = Object::Real(w);
            updated.insert(Name::from(b"Border"), Object::Array(border));
            return AddedKey::None;
        }
    }
    let mut bs = Dict::new();
    bs.insert(Name::from(b"W"), Object::Real(w));
    updated.insert(Name::from(b"BS"), Object::Dict(bs));
    AddedKey::BorderStyle
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
#[allow(clippy::unwrap_used)]
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
        let added = keep_source_keys(&current, None, &mut updated, 1.0);
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
            keep_source_keys(&current, None, &mut updated, 2.0),
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
        let added = keep_source_keys(&current, resolved.as_ref(), &mut updated, 1.0);
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
        let added = keep_source_keys(&current, None, &mut updated, 1.0);
        assert_eq!(added, AddedKey::BorderStyle);
        assert_eq!(keys(&updated), vec![b"Subtype".to_vec(), b"BS".to_vec()]);
    }

    #[test]
    fn a_source_blend_mode_is_kept_and_an_absent_one_stays_absent() {
        let multiply = Object::Name(Name::from(b"Multiply"));
        let with = dict(&[(b"BM", multiply.clone())]);
        let mut updated = with.clone();
        keep_source_keys(&with, None, &mut updated, 1.0);
        assert_eq!(updated.get(b"BM"), Some(&multiply));
        let without = Dict::new();
        let mut updated = dict(&[(b"BM", multiply)]);
        keep_source_keys(&without, None, &mut updated, 1.0);
        assert!(updated.get(b"BM").is_none());
    }
}

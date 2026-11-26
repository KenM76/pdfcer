//! `clone_then_patch`: the one way a carried object's dictionary is built.
//!
//! A carry clones the source dictionary and its closure, then strips a
//! declared key list and applies declared patches. It never assembles a
//! dictionary from an allowlist, so a key nobody thought of is carried by
//! default (decision 228, "Carry rule").
//!
//! # Why a stripped key travels as `null`
//!
//! A *marked* strip leaves `/Key null` in the carried dictionary. ISO
//! 32000-1 §7.3.7 makes a null-valued entry equivalent to an absent one, so
//! a reader of the clip that predates the marker sees nothing different and
//! the clip format version does not change; the paste side, which does know,
//! removes the marker and counts it. That count is what lets a dropped key
//! be disclosed (project rule 4) instead of vanishing silently.

use std::collections::BTreeMap;

use crate::formclip::Closure;
use crate::object::{Dict, Name, Object};
use crate::vector::ClipError;

/// How a stripped key leaves the carried dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Strip {
    /// Removed without trace: the paste always re-supplies it (`/P`).
    Silent,
    /// Replaced by `null` so the paste can count or regenerate it.
    Marked,
}

/// Clone `src` through `closure`, stripping `strip` and then inserting
/// `patch`. A key whose source value is already `null` is treated as absent
/// and is neither carried nor marked.
pub(crate) fn clone_then_patch(
    src: &Dict,
    closure: &mut Closure<'_>,
    strip: &[(&[u8], Strip)],
    patch: Vec<(Name, Object)>,
) -> Result<Dict, ClipError> {
    let mut out = Dict::new();
    for (key, value) in src.iter() {
        if matches!(value, Object::Null) {
            continue;
        }
        match strip.iter().find(|(k, _)| *k == key.as_bytes()) {
            Some((_, Strip::Silent)) => {}
            Some((_, Strip::Marked)) => {
                out.insert(key.clone(), Object::Null);
            }
            None => {
                out.insert(key.clone(), closure.take(value, 0)?);
            }
        }
    }
    for (key, value) in patch {
        out.insert(key, value);
    }
    Ok(out)
}

/// Remove every marked strip of `keys` from `dict`, counting each by name
/// into `dropped`. Returns whether `regenerate` was marked (and removed).
pub(crate) fn take_markers(
    dict: &mut Dict,
    keys: &[&[u8]],
    regenerate: &[u8],
    dropped: &mut BTreeMap<String, u64>,
) -> bool {
    for key in keys {
        if is_marker(dict, key) {
            dict.remove(key);
            *dropped
                .entry(String::from_utf8_lossy(key).into_owned())
                .or_default() += 1;
        }
    }
    let marked = is_marker(dict, regenerate);
    if marked {
        dict.remove(regenerate);
    }
    marked
}

/// Whether `dict` holds `key` as a `null` marker. `Dict::get` reads a null
/// entry as absent (§7.3.7), so the entry is looked up physically.
fn is_marker(dict: &Dict, key: &[u8]) -> bool {
    dict.iter()
        .any(|(k, v)| k.as_bytes() == key && matches!(v, Object::Null))
}

/// What [`take_markers`] would count for `dict`, without changing it.
pub(crate) fn count_markers(dict: &Dict, keys: &[&[u8]], dropped: &mut BTreeMap<String, u64>) {
    let mut probe = dict.clone();
    take_markers(&mut probe, keys, b"", dropped);
}

/// A `/NM` value (§12.5.2 Table 164: unique among the page's annotations)
/// derived from `seed` and absent from `taken`.
pub(crate) fn unique_name(seed: u32, taken: &[Vec<u8>]) -> Vec<u8> {
    let candidate = |n: usize| match n {
        0 => format!("pdfcer-{seed}").into_bytes(),
        _ => format!("pdfcer-{seed}-{n}").into_bytes(),
    };
    // `taken.len() + 1` candidates cannot all be taken.
    (0..=taken.len())
        .map(candidate)
        .find(|c| !taken.contains(c))
        .unwrap_or_else(|| candidate(taken.len() + 1))
}

//! A cut font's subset-coverage lists: `/CIDSet`, a bit per CID, high-order
//! bit first, the first byte's top bit for CID 0 (ISO 32000-1 §9.8.3 Table
//! 124), and `/CharSet`, glyph names each preceded by a slash (§9.8.1 Table
//! 122). Each loses the entries whose glyph the cut emptied; an entry the
//! cutter cannot resolve stays as it was.

use std::collections::{BTreeMap, BTreeSet};

use super::resubset::{FontResubsetter, ResubsetProgram, ResubsetRefusal};
use super::resubset_plan::decoded;
use crate::filters::flate;
use crate::font_unembed::Located;
use crate::fontinfo::FontSubtype;
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::view::DocumentView;

/// A rewritten `/CIDSet` stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CidSetWrite {
    pub(crate) id: ObjId,
    /// The stream data as stored.
    pub(crate) data: Vec<u8>,
    /// Whether [`Self::data`] is Flate-compressed: when the old stream had a
    /// filter.
    pub(crate) flate: bool,
}

/// What a cut rewrites besides the program and the names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Coverage {
    pub(crate) cid_sets: Vec<CidSetWrite>,
    /// Each `/CharSet` string as it is, and as it becomes.
    pub(crate) char_sets: BTreeMap<Vec<u8>, Vec<u8>>,
}

/// The `/CIDSet` and `/CharSet` rewrites of `users`' descriptors after
/// `program`, decoded, is cut to `cut`.
///
/// # Errors
///
/// `cidset-charset-present` when a list cannot be read or rewritten, or the
/// cutter does not resolve its entries.
pub(crate) fn coverage(
    view: &DocumentView<'_>,
    users: &[&Located<'_>],
    source: &dyn FontResubsetter,
    program: &[u8],
    cut: &ResubsetProgram,
) -> Result<Coverage, ResubsetRefusal> {
    let mut out = Coverage::default();
    if users.iter().all(|l| !l.cid_set && !l.char_set) {
        return Ok(out);
    }
    if cut.kept_glyphs.is_empty() {
        return Err(refusal("the font cutter does not say which glyphs it kept"));
    }
    for l in users {
        if l.cid_set {
            let write = cid_set(view, l, source, program, &cut.kept_glyphs)?;
            match out.cid_sets.iter().find(|w| w.id == write.id) {
                Some(w) if *w != write => {
                    return Err(refusal(
                        "two fonts share a /CIDSet they would cut differently",
                    ));
                }
                Some(_) => {}
                None => out.cid_sets.push(write),
            }
        }
        if l.char_set {
            let old = char_set(view, l)?;
            let new = cut_char_set(&old, source, program, &cut.kept_glyphs)?;
            out.char_sets.insert(old, new);
        }
    }
    Ok(out)
}

fn cid_set(
    view: &DocumentView<'_>,
    l: &Located<'_>,
    source: &dyn FontResubsetter,
    program: &[u8],
    kept: &BTreeSet<u16>,
) -> Result<CidSetWrite, ResubsetRefusal> {
    let (Some(id), Some(font), true) = (
        l.cid_set_id,
        l.record.id,
        l.record.subtype == FontSubtype::Type0,
    ) else {
        return Err(refusal(
            "the /CIDSet is not a stream of a Type 0 font's CIDFont",
        ));
    };
    let (_, bits) = decoded(view, id).map_err(|_| refusal("the /CIDSet cannot be read"))?;
    let cids = cids_of(&bits);
    let glyphs = source.cid_glyphs(view, program, font, &cids)?;
    let left: BTreeSet<u32> = cids
        .into_iter()
        .filter(|c| glyphs.get(c).is_none_or(|g| kept.contains(g)))
        .collect();
    let bits = bits_of(&left);
    let flate =
        matches!(view.value(id), Some(Object::Stream(s)) if s.dict.get(b"Filter").is_some());
    Ok(CidSetWrite {
        id,
        data: if flate {
            flate::encode_with_level(&bits, 9)
        } else {
            bits
        },
        flate,
    })
}

/// The CIDs whose bit is set.
pub(crate) fn cids_of(bits: &[u8]) -> BTreeSet<u32> {
    (0u32..)
        .zip(bits)
        .flat_map(|(i, &b)| {
            (0..8u32)
                .filter(move |k| b & (0x80 >> k) != 0)
                .map(move |k| i * 8 + k)
        })
        .collect()
}

/// `cids` as bits, `ceil((max + 1) / 8)` bytes long.
pub(crate) fn bits_of(cids: &BTreeSet<u32>) -> Vec<u8> {
    let len = cids.last().map_or(0, |&m| m as usize / 8 + 1);
    let mut out = vec![0u8; len];
    for &c in cids {
        if let Some(b) = out.get_mut(c as usize / 8) {
            *b |= 0x80 >> (c % 8);
        }
    }
    out
}

/// The `/CharSet` string of `l`'s descriptor.
fn char_set(view: &DocumentView<'_>, l: &Located<'_>) -> Result<Vec<u8>, ResubsetRefusal> {
    let descriptor = descriptor(view, l).ok_or_else(|| refusal("the descriptor cannot be read"))?;
    match descriptor.get(b"CharSet") {
        Some(Object::String(s)) => Ok(s.clone()),
        _ => Err(refusal("the /CharSet is not a direct string")),
    }
}

/// `l`'s font descriptor: on the font, or on a Type 0 font's descendant.
fn descriptor<'v>(view: &'v DocumentView<'_>, l: &Located<'_>) -> Option<&'v Dict> {
    if let Some(id) = l.descriptor_id {
        return view.value(id)?.as_dict();
    }
    let font = view.value(l.record.id?)?.as_dict()?;
    let holder = if l.record.subtype == FontSubtype::Type0 {
        let kids = view.resolve(font.get(b"DescendantFonts")?).as_array()?;
        view.resolve(kids.first()?).as_dict()?
    } else {
        font
    };
    view.resolve(holder.get(b"FontDescriptor")?).as_dict()
}

/// `old` less each name whose glyph is not in `kept`.
fn cut_char_set(
    old: &[u8],
    source: &dyn FontResubsetter,
    program: &[u8],
    kept: &BTreeSet<u16>,
) -> Result<Vec<u8>, ResubsetRefusal> {
    let names: Vec<&[u8]> = old
        .split(|&b| b == b'/' || b.is_ascii_whitespace())
        .filter(|n| !n.is_empty())
        .collect();
    let text: Vec<String> = names
        .iter()
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .collect();
    let glyphs = source.name_glyphs(program, &text)?;
    Ok(names
        .iter()
        .zip(&text)
        .filter(|(_, t)| glyphs.get(*t).is_none_or(|g| kept.contains(g)))
        .flat_map(|(n, _)| std::iter::once(b'/').chain(n.iter().copied()))
        .collect())
}

fn refusal(why: &str) -> ResubsetRefusal {
    ResubsetRefusal::new("cidset-charset-present", why.to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Tests fail loudly.
mod tests {
    use super::*;
    use crate::optimize::resubset::ResubsetRequest;

    #[derive(Debug)]
    struct Names;

    impl FontResubsetter for Names {
        fn resubset(
            &self,
            _: &ResubsetRequest<'_, '_>,
        ) -> Result<ResubsetProgram, ResubsetRefusal> {
            Err(refusal("unused"))
        }

        fn name_glyphs(
            &self,
            _: &[u8],
            names: &[String],
        ) -> Result<BTreeMap<String, u16>, ResubsetRefusal> {
            Ok(names
                .iter()
                .filter_map(|n| {
                    Some((
                        n.clone(),
                        [("a", 1), ("b", 2), ("c", 3)]
                            .iter()
                            .find(|(k, _)| *k == n)?
                            .1,
                    ))
                })
                .collect())
        }
    }

    #[test]
    fn cid_bits_round_trip_high_bit_first() {
        let cids: BTreeSet<u32> = [0, 3, 9].into();
        assert_eq!(bits_of(&cids), [0x90, 0x40]);
        assert_eq!(cids_of(&[0x90, 0x40]), cids);
        assert!(bits_of(&BTreeSet::new()).is_empty());
    }

    #[test]
    fn a_char_set_drops_only_names_of_emptied_glyphs() {
        let kept: BTreeSet<u16> = [0, 1, 3].into();
        let out = cut_char_set(
            b"/a /b/c
/zz",
            &Names,
            b"",
            &kept,
        )
        .unwrap();
        assert_eq!(out, b"/a/c/zz", "b emptied; zz unresolved, so kept");
    }
}

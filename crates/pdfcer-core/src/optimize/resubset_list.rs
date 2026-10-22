//! The `/CIDSet` or `/CharSet` an untagged font owes in a PDF/A-1 document
//! once a cut tags it as a subset (ISO 19005-1 §6.3.5: t2 `/CharSet` on a
//! Type 1 font, t3 `/CIDSet` on a CIDFont).
//!
//! Each list names every glyph the program holds by its structure, never
//! only the drawn ones: PDF/A-1 checkers accept any superset of the used
//! glyphs, and PDF/A-2 and -3 require exactly the present set (ISO 19005-2
//! §6.2.11.4.2), so the full set passes all three. The per-program rules
//! are veraPDF's presence tests (spec RAG
//! `pdfa/pdfa__ref__font_subset_cidset_charset.md` §3.2-3.3).

use std::collections::BTreeSet;

use super::resubset::{
    FontResubsetter, PresentGlyphs, ResubsetPlan, ResubsetRefusal, ResubsetTarget,
};
use super::resubset_plan::refused;
use crate::filters::flate;
use crate::font_unembed::Located;
use crate::fontinfo::FontSubtype;
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::view::DocumentView;

/// One descriptor and the list it gains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwedList {
    pub(crate) descriptor: ObjId,
    pub(crate) entry: ListEntry,
}

/// A list as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ListEntry {
    /// `/CIDSet` stream data: an MSB-first bit per CID, CID 0 first
    /// (ISO 32000-1 Table 124).
    CidSet(Vec<u8>),
    /// The `/CharSet` string: slash-prefixed names, `.notdef` omitted
    /// (ISO 32000-1 Table 122).
    CharSet(Vec<u8>),
}

/// Whether `l`, untagged now, would become a PDF/A-1 subset without the
/// list ISO 19005-1 §6.3.5 requires. A tagged font keeps whatever it had,
/// which the cut leaves true.
pub(super) fn owes_a_list(l: &Located<'_>) -> bool {
    if l.record.subset_tag.is_some() {
        return false;
    }
    match l.record.subtype {
        FontSubtype::Type0 | FontSubtype::CidFontType0 | FontSubtype::CidFontType2 => !l.cid_set,
        FontSubtype::Type1 | FontSubtype::MmType1 => !l.char_set,
        _ => false,
    }
}

/// Give every target the lists its fonts owe; a target one cannot be made
/// for moves to the refusals.
pub(super) fn attach(
    view: &DocumentView<'_>,
    located: &[Located<'_>],
    source: &dyn FontResubsetter,
    out: &mut ResubsetPlan,
) {
    for mut t in std::mem::take(&mut out.targets) {
        let users: Vec<&Located<'_>> = t
            .fonts
            .iter()
            .filter_map(|&f| located.iter().find(|l| l.record.id == Some(f)))
            .collect();
        match lists(view, &users, source, &t) {
            Ok(lists) => {
                t.listed = lists.iter().map(|o| o.descriptor).collect();
                t.lists = lists;
                out.targets.push(t);
            }
            Err(why) => out
                .refused
                .extend(users.iter().map(|l| refused(l, why.clone()))),
        }
    }
}

/// The lists `t`'s `users` owe, one per descriptor.
fn lists(
    view: &DocumentView<'_>,
    users: &[&Located<'_>],
    source: &dyn FontResubsetter,
    t: &ResubsetTarget,
) -> Result<Vec<OwedList>, ResubsetRefusal> {
    let owing: Vec<&&Located<'_>> = users.iter().filter(|l| owes_a_list(l)).collect();
    if owing.is_empty() {
        return Ok(Vec::new());
    }
    let program =
        flate::decode(&t.data, None).map_err(|_| owed("the cut program could not be read back"))?;
    let present = source.present_glyphs(&program)?;
    let mut out: Vec<OwedList> = Vec::new();
    for l in owing {
        // The planner snapshots every user's descriptor; one it did not
        // would have its list dropped at apply, so refuse instead.
        let descriptor = l
            .descriptor_id
            .filter(|d| t.before.iter().any(|(id, _)| id == d))
            .ok_or_else(|| owed("its font descriptor is not an indirect object"))?;
        let entry = entry(view, l, &present)?;
        if !out.iter().any(|o| o.descriptor == descriptor) {
            out.push(OwedList { descriptor, entry });
        }
    }
    Ok(out)
}

/// The list `l` owes, given what its program holds.
fn entry(
    view: &DocumentView<'_>,
    l: &Located<'_>,
    present: &PresentGlyphs,
) -> Result<ListEntry, ResubsetRefusal> {
    if matches!(l.record.subtype, FontSubtype::Type1 | FontSubtype::MmType1) {
        return match present {
            PresentGlyphs::Names(names) => char_set(names).map(ListEntry::CharSet),
            _ => Err(owed("its program is not a name-keyed CFF program")),
        };
    }
    let cid_font = cid_font(view, l).ok_or_else(|| owed("its CIDFont cannot be read"))?;
    let subtype = cid_font.get(b"Subtype").and_then(Object::as_name);
    let cids = match (subtype.map(|n| n.as_bytes()), present) {
        (Some(b"CIDFontType2"), PresentGlyphs::Count(n)) => truetype_cids(view, &cid_font, *n)?,
        (Some(b"CIDFontType0"), PresentGlyphs::Cids(cids)) => cids.clone(),
        (Some(b"CIDFontType0"), PresentGlyphs::Names(_)) => BTreeSet::from([0]),
        _ => return Err(owed("its program does not match its CIDFont type")),
    };
    Ok(ListEntry::CidSet(bits(&cids)))
}

/// The CIDFont `l` draws with: its descendant, or the font itself.
fn cid_font(view: &DocumentView<'_>, l: &Located<'_>) -> Option<Dict> {
    let font = view.value(l.record.id?)?.as_dict()?;
    if l.record.subtype != FontSubtype::Type0 {
        return Some(font.clone());
    }
    match view.resolve(font.get(b"DescendantFonts")?) {
        Object::Array(items) => view.resolve(items.first()?).as_dict().cloned(),
        _ => None,
    }
}

/// The CIDs a CIDFontType2 over `num_glyphs` glyphs holds: every CID below
/// `num_glyphs` under an `/Identity` (or absent) `/CIDToGIDMap`, else every
/// CID the map stream covers whose glyph exists, including one mapped to
/// glyph 0.
fn truetype_cids(
    view: &DocumentView<'_>,
    cid_font: &Dict,
    num_glyphs: u32,
) -> Result<BTreeSet<u32>, ResubsetRefusal> {
    let map = cid_font.get(b"CIDToGIDMap").map(|o| view.resolve(o));
    let stream = match map {
        None => return Ok((0..num_glyphs).collect()),
        Some(Object::Name(n)) if n.as_bytes() == b"Identity" => {
            return Ok((0..num_glyphs).collect());
        }
        Some(Object::Stream(s)) => s,
        _ => return Err(owed("its /CIDToGIDMap cannot be read")),
    };
    let unreadable = || owed("its /CIDToGIDMap cannot be read");
    let raw = view.slice(stream.data_span).ok_or_else(unreadable)?;
    let data = crate::filters::decode_stream(&stream.dict, raw).map_err(|_| unreadable())?;
    let mut out = BTreeSet::from([0]);
    for (cid, gid) in (0u32..).zip(data.chunks_exact(2)) {
        if let [hi, lo] = *gid
            && u32::from(u16::from_be_bytes([hi, lo])) < num_glyphs
        {
            out.insert(cid);
        }
    }
    Ok(out)
}

/// `cids` as a `/CIDSet` bit table: bit 7 of byte 0 is CID 0.
fn bits(cids: &BTreeSet<u32>) -> Vec<u8> {
    let last = cids.last().copied().unwrap_or(0);
    let mut out = vec![0u8; usize::try_from(last / 8 + 1).unwrap_or(0)];
    for &cid in cids {
        if let Some(byte) = usize::try_from(cid / 8).ok().and_then(|i| out.get_mut(i)) {
            *byte |= 0x80 >> (cid % 8);
        }
    }
    out
}

/// `names` as a `/CharSet` string. A name that a `/` sequence cannot carry
/// (a delimiter, white space or a byte outside `!`..`~`) refuses.
fn char_set(names: &BTreeSet<Vec<u8>>) -> Result<Vec<u8>, ResubsetRefusal> {
    let mut out = Vec::new();
    for name in names.iter().filter(|n| n.as_slice() != b".notdef") {
        let regular = |&b: &u8| (b'!'..=b'~').contains(&b) && !b"()<>[]{}/%".contains(&b);
        if name.is_empty() || !name.iter().all(regular) {
            return Err(owed("a glyph name cannot be written in a /CharSet"));
        }
        out.push(b'/');
        out.extend_from_slice(name);
    }
    Ok(out)
}

fn owed(why: &str) -> ResubsetRefusal {
    ResubsetRefusal::new(
        "pdfa1-list-owed",
        format!(
            "the document claims PDF/A-1, which requires a /CIDSet or /CharSet on a subset font; this font has none, and pdfcer cannot write one: {why}"
        ),
    )
}

#[cfg(test)]
#[path = "resubset_list_tests.rs"]
mod tests;
